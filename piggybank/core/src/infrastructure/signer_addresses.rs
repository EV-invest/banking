//! Signer-backed deposit-address adapter — the production [`DepositAddresses`].
//!
//! On first use for a `(user, network)` it asks the
//! separate-process **signer** to generate the curve keypair, seal the private key, and
//! return the public address. The hub never sees the key — only the address and its
//! `address_kind`, which it caches in `user_deposit_addresses`. Subsequent reads hit that
//! cache and never touch the signer, so the watch-only read path stays cheap.
//!
//! The kind gates fundability. Until real pubkey→address encoding ships the signer
//! returns a `placeholder` (a structurally-valid string bound to the key, but NOT its
//! on-chain image). A placeholder is NEVER returned as a fundable address — [`address`]
//! yields `None` and the wallet view marks the rail unavailable. Because a cached
//! placeholder must not become a permanent trap, a still-`placeholder` row re-asks the
//! signer and **backfills** itself in place once the signer reports `derived` (the signer
//! recomputes from its stored `public_key` + `key_version`, so the same key keeps the
//! same address). Once `derived`, reads short-circuit without contacting the signer.
//!
//! [`address`]: SignerDepositAddresses::address

use async_trait::async_trait;
use domain::{
	architecture::Gateway,
	error::DomainError,
	money::{Network, WalletAddress},
	users::UserId,
};
use evbanking_auth::ServiceTokenSource;
use evbanking_contracts::signer::v1::{MigrateAddressToCustodianRequest, ProvisionAddressRequest, RotateAddressRequest, signer_service_client::SignerServiceClient};
use sqlx::PgPool;
use tonic::{Request, transport::Channel};

use crate::ports::deposit_addresses::{DepositAddresses, MigratedAddress};

const KIND_DERIVED: &str = "derived";

pub struct SignerDepositAddresses {
	pool: PgPool,
	client: SignerServiceClient<Channel>,
	/// Authenticates the hub's onward calls to the now-authenticated signer seam with a
	/// `typ=service` token (`aud=banking-services`). `None` in unconfigured dev/CI: the
	/// signer then rejects the call, but the address path is unreachable there anyway
	/// (no auth ⇒ no client can request an address in the first place).
	service_token: Option<ServiceTokenSource>,
}

impl SignerDepositAddresses {
	pub fn new(pool: PgPool, client: SignerServiceClient<Channel>, service_token: Option<ServiceTokenSource>) -> Self {
		Self { pool, client, service_token }
	}

	/// Ask the signer to (idempotently) provision the address and cache it with its kind.
	/// An existing row is backfilled in place — so a cached placeholder is upgraded to the
	/// real `derived` address the moment the signer can compute it, never left stale.
	async fn provision_and_cache(&self, user: UserId, network: Network) -> Result<(WalletAddress, bool), DomainError> {
		let mut request = Request::new(ProvisionAddressRequest {
			user_id: user.raw().to_string(),
			network: network.as_str().to_owned(),
		});
		if let Some(token) = &self.service_token {
			request = token.authorize(request);
		}
		let response = self
			.client
			.clone()
			.provision_address(request)
			.await
			.map_err(|status| DomainError::Repository(format!("signer provision failed: {}", status.message())))?
			.into_inner();
		let address = WalletAddress::parse(network, &response.address)?;
		let derived = response.address_kind == KIND_DERIVED;
		self.cache(user, network, &address, derived).await?;
		Ok((address, derived))
	}

	/// Upsert the hub's watch-only cache row — the address `GetDepositAddress` serves
	/// and the deposit watcher / sweep read.
	async fn cache(&self, user: UserId, network: Network, address: &WalletAddress, derived: bool) -> Result<(), DomainError> {
		let kind = if derived { KIND_DERIVED } else { "placeholder" };
		sqlx::query(
			"INSERT INTO user_deposit_addresses (user_id, network, address, address_kind) VALUES ($1, $2, $3, $4) \
			 ON CONFLICT (user_id, network) DO UPDATE SET address = EXCLUDED.address, address_kind = EXCLUDED.address_kind",
		)
		.bind(user.raw())
		.bind(network.as_str())
		.bind(address.as_str())
		.bind(kind)
		.execute(&self.pool)
		.await
		.map_err(repo_err)?;
		Ok(())
	}
}

impl Gateway for SignerDepositAddresses {}

#[async_trait]
impl DepositAddresses for SignerDepositAddresses {
	async fn address(&self, user: UserId, network: Network) -> Result<Option<WalletAddress>, DomainError> {
		// Fast path: a cached `derived` address is fundable and returned without the signer.
		// A cached `placeholder` is NOT served — it falls through to a recompute attempt.
		if let Some((address, kind)) = sqlx::query_as::<_, (String, String)>("SELECT address, address_kind FROM user_deposit_addresses WHERE user_id = $1 AND network = $2")
			.bind(user.raw())
			.bind(network.as_str())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo_err)?
			&& kind == KIND_DERIVED
		{
			return Ok(Some(WalletAddress::parse(network, &address)?));
		}

		// No row yet, or a cached placeholder to recompute: the signer mints/seals on first
		// call and re-derives from its stored key thereafter (idempotent per user+network).
		let (address, derived) = self.provision_and_cache(user, network).await?;
		Ok(derived.then_some(address))
	}

	/// Matched case-insensitively: EVM addresses are stored checksummed but arrive from the
	/// chain lowercased, and a case mismatch here would read as "not our address" and refuse
	/// a real deposit. Only `derived` rows count — a placeholder is not an on-chain image of
	/// any key, so nothing can have been sent to it.
	async fn owner_of(&self, network: Network, address: &str) -> Result<Option<UserId>, DomainError> {
		let owner: Option<uuid::Uuid> = sqlx::query_scalar("SELECT user_id FROM user_deposit_addresses WHERE network = $1 AND lower(address) = lower($2) AND address_kind = 'derived'")
			.bind(network.as_str())
			.bind(address)
			.fetch_optional(&self.pool)
			.await
			.map_err(repo_err)?;
		Ok(owner.map(UserId::from_raw))
	}

	async fn rotate(&self, user: UserId, network: Network) -> Result<WalletAddress, DomainError> {
		let mut request = Request::new(RotateAddressRequest {
			user_id: user.raw().to_string(),
			network: network.as_str().to_owned(),
		});
		if let Some(token) = &self.service_token {
			request = token.authorize(request);
		}
		let response = self
			.client
			.clone()
			.rotate_address(request)
			.await
			.map_err(|status| match status.code() {
				// The signer's refusals (healthy key / nothing to rotate) are operator
				// input errors, not infrastructure faults.
				tonic::Code::FailedPrecondition | tonic::Code::InvalidArgument => DomainError::Validation(format!("signer refused rotation: {}", status.message())),
				_ => DomainError::Repository(format!("signer rotation failed: {}", status.message())),
			})?
			.into_inner();
		let address = WalletAddress::parse(network, &response.address)?;
		let derived = response.address_kind == KIND_DERIVED;
		// Refresh the cache IMMEDIATELY: the fast path in `address` short-circuits on a
		// cached derived row, so without this the hub would keep serving the dead address.
		self.cache(user, network, &address, derived).await?;
		if !derived {
			return Err(DomainError::Repository("rotated address is not derived (signer reported a placeholder)".into()));
		}
		Ok(address)
	}

	async fn migrate_to_custodian(&self, user: UserId, network: Network, drained_address: &str) -> Result<MigratedAddress, DomainError> {
		let mut request = Request::new(MigrateAddressToCustodianRequest {
			user_id: user.raw().to_string(),
			network: network.as_str().to_owned(),
			drained_address: drained_address.to_owned(),
		});
		if let Some(token) = &self.service_token {
			request = token.authorize(request);
		}
		let response = self
			.client
			.clone()
			.migrate_address_to_custodian(request)
			.await
			.map_err(|status| match status.code() {
				// The signer's own gates (already custody-held, dead key, a different address
				// than the one cleared) are operator input errors, not infrastructure faults —
				// same split `rotate` makes. `Aborted` is a lost race: the row moved under us,
				// and the honest answer is "re-read and try again", not "the signer is broken".
				tonic::Code::FailedPrecondition | tonic::Code::InvalidArgument | tonic::Code::Aborted => {
					DomainError::Validation(format!("signer refused the custody migration: {}", status.message()))
				}
				_ => DomainError::Repository(format!("signer custody migration failed: {}", status.message())),
			})?
			.into_inner();
		let new_address = WalletAddress::parse(network, &response.new_address)?;
		let derived = response.address_kind == KIND_DERIVED;
		// Refresh the cache IMMEDIATELY, exactly as `rotate` does and for the same reason: the
		// fast path in `address` short-circuits on a cached derived row, so until this lands
		// the hub keeps serving — and the watchers keep watching — the retired address.
		self.cache(user, network, &new_address, derived).await?;
		if !derived {
			return Err(DomainError::Repository("migrated address is not derived (signer reported a placeholder)".into()));
		}
		Ok(MigratedAddress {
			old_address: response.old_address,
			new_address,
		})
	}
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

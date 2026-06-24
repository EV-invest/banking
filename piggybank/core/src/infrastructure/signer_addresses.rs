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
	error::DomainError,
	money::{Network, WalletAddress},
	users::UserId,
};
use evbanking_contracts::signer::v1::{ProvisionAddressRequest, signer_service_client::SignerServiceClient};
use sqlx::PgPool;
use tonic::transport::Channel;

use crate::ports::deposit_addresses::DepositAddresses;

const KIND_DERIVED: &str = "derived";

pub struct SignerDepositAddresses {
	pool: PgPool,
	client: SignerServiceClient<Channel>,
}

impl SignerDepositAddresses {
	pub fn new(pool: PgPool, client: SignerServiceClient<Channel>) -> Self {
		Self { pool, client }
	}

	/// Ask the signer to (idempotently) provision the address and cache it with its kind.
	/// An existing row is backfilled in place — so a cached placeholder is upgraded to the
	/// real `derived` address the moment the signer can compute it, never left stale.
	async fn provision_and_cache(&self, user: UserId, network: Network) -> Result<(WalletAddress, bool), DomainError> {
		let response = self
			.client
			.clone()
			.provision_address(ProvisionAddressRequest {
				user_id: user.raw().to_string(),
				network: network.as_str().to_owned(),
			})
			.await
			.map_err(|status| DomainError::Repository(format!("signer provision failed: {}", status.message())))?
			.into_inner();
		let address = WalletAddress::parse(network, &response.address)?;
		let derived = response.address_kind == KIND_DERIVED;
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
		Ok((address, derived))
	}
}

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
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

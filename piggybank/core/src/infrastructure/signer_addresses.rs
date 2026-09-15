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
use tonic::{Code, Request, Status, transport::Channel};

use crate::ports::deposit_addresses::{DepositAddresses, MigratedAddress};

const KIND_DERIVED: &str = "derived";

/// The trailer the signer attaches to a `FailedPrecondition` when the key custodian parked
/// the activity for a human (`BackendError::RequiresApproval`). The hub does not depend on
/// the signer crate, so this mirrors its `ACTIVITY_ID_METADATA_KEY` rather than importing it.
const ACTIVITY_ID_METADATA_KEY: &str = "turnkey-activity-id";

/// Classify a signer refusal of `op` (a rotation or a custody migration) into the domain
/// vocabulary, so the operator's wire code tells the cases apart.
///
/// `FailedPrecondition` is the signer saying "not in this state" — a healthy key that must
/// not be rotated, a row that is already custody-held, or the custodian holding the activity
/// for human approval. Only the state stands in the way, which is exactly
/// [`DomainError::Precondition`]; collapsing it into `Validation` (as this once did) told an
/// operator to fix a request that was fine. The approval case carries the custodian's
/// activity id in the `turnkey-activity-id` trailer; the domain error cannot carry metadata
/// and the trailer is deliberately not forwarded to clients, so the id goes into the message,
/// where the holder of `DepositAddressRotate`/`DepositAddressMigrate` can read it off the
/// status and find the activity on the custodian's side.
///
/// `PermissionDenied` is the custodian refusing ON THE MERITS (`BackendError::Rejected`: its
/// policy said no to minting the key) — terminal for this request, and the signer composes
/// the message itself, never from a vendor body, so it is safe to show. It must not fall
/// into `Repository`: `map_err` hides that as a retryable `Unavailable("internal error")`,
/// and the operator would retry a refusal with no idea why. `Forbidden` keeps both the code
/// and the reason. `InvalidArgument` is a malformed request. `Aborted` is a lost race: the
/// row moved under us, and the honest answer is "re-read and try again", not "the signer
/// is broken". Anything else is an infrastructure fault.
fn signer_refusal(op: &str, status: &Status) -> DomainError {
	let message = status.message();
	match status.code() {
		Code::FailedPrecondition => match activity_id(status) {
			// The signer's message already names the activity; the prefix is what makes the
			// case recognisable without knowing that message's wording.
			Some(_) => DomainError::Precondition(format!("signer {op} requires custodian approval: {message}")),
			None => DomainError::Precondition(format!("signer refused the {op}: {message}")),
		},
		Code::PermissionDenied => DomainError::Forbidden(format!("signer refused the {op}: {message}")),
		Code::InvalidArgument | Code::Aborted => DomainError::Validation(format!("signer refused the {op}: {message}")),
		_ => DomainError::Repository(format!("signer {op} failed: {message}")),
	}
}

fn activity_id(status: &Status) -> Option<&str> {
	status.metadata().get(ACTIVITY_ID_METADATA_KEY).and_then(|value| value.to_str().ok())
}

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
			.map_err(|status| signer_refusal("rotation", &status))?
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
			.map_err(|status| signer_refusal("custody migration", &status))?
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

#[cfg(test)]
mod tests {
	use tonic::metadata::MetadataValue;

	use super::*;

	const ACTIVITY_ID: &str = "0f6a2b3c-4d5e-4f70-8a9b-0c1d2e3f4a5b";

	fn requires_approval() -> Status {
		let mut status = Status::failed_precondition(format!("key custodian requires approval for activity {ACTIVITY_ID}"));
		status.metadata_mut().insert(ACTIVITY_ID_METADATA_KEY, MetadataValue::try_from(ACTIVITY_ID).unwrap());
		status
	}

	#[test]
	fn requires_approval_is_a_precondition_that_names_the_activity() {
		let err = signer_refusal("rotation", &requires_approval());
		let DomainError::Precondition(message) = err else {
			panic!("RequiresApproval must be a precondition, not validation: {err:?}");
		};
		assert!(message.contains(ACTIVITY_ID), "the activity id must survive into the message: {message}");
		assert!(message.contains("approval"), "{message}");
	}

	#[test]
	fn a_plain_failed_precondition_is_a_precondition_without_an_activity() {
		let err = signer_refusal("rotation", &Status::failed_precondition("key is healthy under the current KEK — rotation refused"));
		let DomainError::Precondition(message) = err else {
			panic!("a signer precondition must not collapse into validation: {err:?}");
		};
		assert!(!message.contains("activity"), "no trailer ⇒ no invented activity id: {message}");
		assert!(message.contains("rotation refused"), "{message}");
	}

	#[test]
	fn bad_input_and_lost_races_stay_validation() {
		assert!(matches!(
			signer_refusal("custody migration", &Status::invalid_argument("user_id must be a UUID")),
			DomainError::Validation(_)
		));
		assert!(matches!(
			signer_refusal("custody migration", &Status::aborted("superseded concurrently")),
			DomainError::Validation(_)
		));
	}

	#[test]
	fn a_custodian_refusal_on_the_merits_is_forbidden_and_keeps_its_reason() {
		let err = signer_refusal("custody migration", &Status::permission_denied("key custodian refused: policy denied CREATE_WALLET_ACCOUNTS"));
		let DomainError::Forbidden(message) = err else {
			panic!("a refusal on the merits must not hide behind a retryable internal error: {err:?}");
		};
		assert!(message.contains("policy denied"), "the reason must reach the operator: {message}");
	}

	#[test]
	fn everything_else_is_an_infrastructure_fault() {
		for status in [
			Status::unavailable("down"),
			Status::deadline_exceeded("slow"),
			Status::internal("signing failed"),
			Status::unauthenticated("token"),
		] {
			let code = status.code();
			assert!(matches!(signer_refusal("rotation", &status), DomainError::Repository(_)), "{code:?} must be a repository error");
		}
	}
}

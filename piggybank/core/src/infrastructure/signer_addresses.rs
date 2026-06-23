//! Signer-backed deposit-address adapter — the production [`DepositAddresses`].
//!
//! Replaces the [`StubDepositAddresses`](super::deposit_addresses::StubDepositAddresses)
//! derivation with real provisioning: on first use for a `(user, network)` it asks the
//! separate-process **signer** to generate the curve keypair, seal the private key, and
//! return the public address. The hub never sees the key — only the address, which it
//! caches in `user_deposit_addresses` exactly as before. Subsequent reads hit that cache
//! and never touch the signer, so the watch-only read path is unchanged.

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

pub struct SignerDepositAddresses {
	pool: PgPool,
	client: SignerServiceClient<Channel>,
}

impl SignerDepositAddresses {
	pub fn new(pool: PgPool, client: SignerServiceClient<Channel>) -> Self {
		Self { pool, client }
	}
}

#[async_trait]
impl DepositAddresses for SignerDepositAddresses {
	async fn address(&self, user: UserId, network: Network) -> Result<WalletAddress, DomainError> {
		// Watch-only fast path: a cached address is returned without contacting the signer.
		if let Some(existing) = sqlx::query_scalar::<_, String>("SELECT address FROM user_deposit_addresses WHERE user_id = $1 AND network = $2")
			.bind(user.raw())
			.bind(network.as_str())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo_err)?
		{
			return WalletAddress::parse(network, &existing);
		}

		// First time: the signer mints + seals the keypair and hands back the address
		// (idempotent on its side per (user, network)).
		let response = self
			.client
			.clone()
			.provision_address(ProvisionAddressRequest {
				user_id: user.raw().to_string(),
				network: network.as_str().to_owned(),
			})
			.await
			.map_err(|status| DomainError::Repository(format!("signer provision failed: {}", status.message())))?;
		let address = WalletAddress::parse(network, &response.into_inner().address)?;

		// Cache the watch-only address so future reads skip the signer.
		sqlx::query("INSERT INTO user_deposit_addresses (user_id, network, address) VALUES ($1, $2, $3) ON CONFLICT (user_id, network) DO NOTHING")
			.bind(user.raw())
			.bind(network.as_str())
			.bind(address.as_str())
			.execute(&self.pool)
			.await
			.map_err(repo_err)?;
		Ok(address)
	}
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

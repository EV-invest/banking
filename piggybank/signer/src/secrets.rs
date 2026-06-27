//! The `wallet_secrets` store — the signer's private control plane.
//!
//! Holds the **sealed** chain private keys (output of [`Vault::seal`](crate::key_vault::Vault::seal),
//! `nonce||ciphertext`) plus the public key/address for watch-only indexing and a
//! `key_alg`/`key_version` tag for future rotation. Lives in the signer's OWN
//! database; the hub has no access.
//!
//! Least privilege: the provisioning path only ever writes a row and reads the
//! address back. The sealed blob is loaded ONLY by [`find_sealed`](WalletSecrets::find_sealed)
//! — the signer-internal signing path (a later feature) — never by an ordinary read.

use domain::money::Network;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::SignerError;

/// A new sealed-key row to persist. Borrows its byte payloads so the caller keeps
/// ownership of (and can promptly drop/zeroize) the sensitive material.
pub struct NewSecret<'a> {
	pub id: Uuid,
	pub user_id: Uuid,
	pub network: Network,
	pub public_key: &'a [u8],
	pub address: &'a str,
	pub sealed_key: &'a [u8],
	pub key_alg: &'a str,
	pub key_version: i32,
}

/// A sealed row read back for signing. The blob is still ciphertext — opening it
/// needs the [`Vault`](crate::key_vault::Vault) plus this row's `id` (the AAD
/// `wallet_id`).
pub struct SealedSecret {
	pub id: Uuid,
	pub network: Network,
	pub key_alg: String,
	pub key_version: i32,
	pub sealed_key: Vec<u8>,
}

pub struct WalletSecrets {
	pool: PgPool,
}

impl WalletSecrets {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}

	/// Insert a sealed key, idempotent per `(user_id, network)`: a concurrent racer
	/// that inserted first wins and this is a no-op (the caller re-reads the canonical
	/// address). Never overwrites an existing key.
	pub async fn insert(&self, secret: &NewSecret<'_>) -> Result<(), SignerError> {
		sqlx::query(
			"INSERT INTO wallet_secrets (id, user_id, network, public_key, address, sealed_key, key_alg, key_version) \
			 VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (user_id, network) DO NOTHING",
		)
		.bind(secret.id)
		.bind(secret.user_id)
		.bind(secret.network.as_str())
		.bind(secret.public_key)
		.bind(secret.address)
		.bind(secret.sealed_key)
		.bind(secret.key_alg)
		.bind(secret.key_version)
		.execute(&self.pool)
		.await?;
		Ok(())
	}

	/// The watch-only address for `(user, network)`, if provisioned. Does NOT touch
	/// the sealed blob — the ordinary read path.
	pub async fn find_address(&self, user_id: Uuid, network: Network) -> Result<Option<String>, SignerError> {
		let address = sqlx::query_scalar::<_, String>("SELECT address FROM wallet_secrets WHERE user_id = $1 AND network = $2")
			.bind(user_id)
			.bind(network.as_str())
			.fetch_optional(&self.pool)
			.await?;
		Ok(address)
	}

	/// The watch-only `(address, public_key)` for `(user, network)`, if provisioned. Public
	/// data only — never the sealed blob; used to (re-)derive the on-chain address.
	pub async fn find_watch(&self, user_id: Uuid, network: Network) -> Result<Option<(String, Vec<u8>)>, SignerError> {
		let row = sqlx::query_as::<_, (String, Vec<u8>)>("SELECT address, public_key FROM wallet_secrets WHERE user_id = $1 AND network = $2")
			.bind(user_id)
			.bind(network.as_str())
			.fetch_optional(&self.pool)
			.await?;
		Ok(row)
	}

	/// Backfill the stored watch-only address in place — e.g. a placeholder upgraded to the
	/// real derived address once a chain's encoding lands. Never touches the key material.
	pub async fn update_address(&self, user_id: Uuid, network: Network, address: &str) -> Result<(), SignerError> {
		sqlx::query("UPDATE wallet_secrets SET address = $3 WHERE user_id = $1 AND network = $2")
			.bind(user_id)
			.bind(network.as_str())
			.bind(address)
			.execute(&self.pool)
			.await?;
		Ok(())
	}

	/// Load the SEALED private key for `(user, network)` — the signer-only path used
	/// by signing (and the round-trip test). The blob is still encrypted; the caller
	/// opens it transiently in the [`Vault`](crate::key_vault::Vault) and zeroizes it.
	pub async fn find_sealed(&self, user_id: Uuid, network: Network) -> Result<Option<SealedSecret>, SignerError> {
		let row = sqlx::query_as::<_, (Uuid, String, String, i32, Vec<u8>)>("SELECT id, network, key_alg, key_version, sealed_key FROM wallet_secrets WHERE user_id = $1 AND network = $2")
			.bind(user_id)
			.bind(network.as_str())
			.fetch_optional(&self.pool)
			.await?;
		row.map(|(id, network, key_alg, key_version, sealed_key)| {
			let network = Network::parse(&network).map_err(|e| SignerError::Repository(e.to_string()))?;
			Ok(SealedSecret {
				id,
				network,
				key_alg,
				key_version,
				sealed_key,
			})
		})
		.transpose()
	}
}

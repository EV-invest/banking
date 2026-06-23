use std::{env, net::SocketAddr};

use anyhow::Context;

use crate::key_vault::Vault;

/// Signer configuration, sourced from environment variables (and `.env` in
/// development via `dotenvy`). The KEK is **not** here on purpose — it never sits
/// in a `Debug`-printable struct; [`load_vault`] reads it and hands back a
/// [`Vault`] that holds it zeroized.
#[derive(Clone, Debug)]
pub struct SignerConfig {
	/// The signer's OWN database (separate from the hub's): holds `wallet_secrets`,
	/// the sealed private keys. Distinct credentials so a hub-side compromise can't
	/// even read the (already-encrypted) blobs.
	pub database_url: String,
	/// gRPC listener for the internal hub↔signer seam.
	pub grpc_addr: SocketAddr,
}

impl SignerConfig {
	pub fn from_env() -> anyhow::Result<Self> {
		let database_url = env::var("SIGNER_DATABASE_URL").context("SIGNER_DATABASE_URL must be set")?;
		let grpc_addr = env::var("SIGNER_GRPC_ADDR")
			.unwrap_or_else(|_| "0.0.0.0:50053".to_string())
			.parse()
			.context("SIGNER_GRPC_ADDR must be a valid socket address, e.g. 0.0.0.0:50053")?;
		Ok(Self { database_url, grpc_addr })
	}
}

/// Load the key-encrypting key from `WALLET_KEK` (64 hex chars / 32 bytes) and
/// build the [`Vault`]. **Fail-fast**: the signer refuses to start without a valid
/// KEK, so it can never silently run unable to seal/open. The KEK is injected from
/// outside (secrets manager / KMS); it must never live in the DB, repo, or a config
/// file next to the ciphertext.
pub fn load_vault() -> anyhow::Result<Vault> {
	let kek_hex = env::var("WALLET_KEK").context("WALLET_KEK must be set (64 hex chars = 32 bytes), injected from a secrets store")?;
	Vault::from_hex(&kek_hex).context("WALLET_KEK is not a valid 32-byte hex key")
}

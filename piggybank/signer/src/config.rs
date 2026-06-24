use std::{env, net::SocketAddr};

use anyhow::Context;
use evbanking_auth::{TokenType, VerifierConfig};

use crate::key_vault::Vault;

/// Default bind: loopback, so the hub↔signer seam is not exposed on every interface
/// out of the box. The signer is a distinct trust domain holding the chain keys; a
/// wider bind is an explicit opt-in that REQUIRES TLS (see [`SignerConfig::from_env`]).
const DEFAULT_GRPC_ADDR: &str = "127.0.0.1:50053";

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
	/// Inbound authentication: the seam accepts only the hub's service token
	/// (`aud=banking-services`, `typ=service`), verified locally against the auth
	/// service's JWKS. The signer is a separate trust domain — network reachability
	/// is NOT the authorization boundary.
	pub verifier: VerifierConfig,
	/// Server TLS for the seam. Required (fail-fast) whenever [`grpc_addr`] is
	/// non-loopback; `None` is allowed only on a loopback bind. The optional client-CA
	/// root upgrades it to mTLS, pinning the hub's client certificate.
	///
	/// [`grpc_addr`]: SignerConfig::grpc_addr
	pub tls: Option<TlsConfig>,
}
impl SignerConfig {
	pub fn from_env() -> anyhow::Result<Self> {
		let database_url = env::var("SIGNER_DATABASE_URL").context("SIGNER_DATABASE_URL must be set")?;
		let grpc_addr: SocketAddr = env::var("SIGNER_GRPC_ADDR")
			.unwrap_or_else(|_| DEFAULT_GRPC_ADDR.to_string())
			.parse()
			.with_context(|| format!("SIGNER_GRPC_ADDR must be a valid socket address, e.g. {DEFAULT_GRPC_ADDR}"))?;

		// The seam verifies the hub's service token, not a client access token: pin the
		// service audience + `typ=service` so a user/client token can never drive it.
		let verifier = VerifierConfig {
			issuer: env::var("AUTH_ISSUER").unwrap_or_else(|_| "https://auth.banking.ev".to_string()),
			audiences: split_csv(&env::var("AUTH_SERVICE_AUDIENCE").unwrap_or_else(|_| "banking-services".to_string())),
			allowed_types: vec![TokenType::Service],
			jwks_grpc_endpoint: env::var("AUTH_JWKS_GRPC_ENDPOINT").context("AUTH_JWKS_GRPC_ENDPOINT must be set so the signer can verify the hub's service token")?,
		};

		let tls = load_tls()?;
		// Fail closed: a non-loopback bind in cleartext would expose the seam off-host, so
		// the safe default (loopback) is the only case that may run without TLS.
		if !grpc_addr.ip().is_loopback() && tls.is_none() {
			anyhow::bail!(
				"SIGNER_GRPC_ADDR {grpc_addr} is non-loopback but no TLS is configured — set SIGNER_TLS_CERT_PEM_FILE/SIGNER_TLS_KEY_PEM_FILE (mTLS recommended) or bind to loopback"
			);
		}

		Ok(Self {
			database_url,
			grpc_addr,
			verifier,
			tls,
		})
	}
}

/// Server-side TLS material for the seam, read from PEM files on disk.
#[derive(Clone, Debug)]
pub struct TlsConfig {
	pub cert_pem: String,
	pub key_pem: String,
	/// Trust root for verifying the hub's client certificate (mTLS). `None` ⇒ TLS only.
	pub client_ca_pem: Option<String>,
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

/// Load the server TLS material, if `SIGNER_TLS_CERT_PEM_FILE` and
/// `SIGNER_TLS_KEY_PEM_FILE` are both set; the optional `SIGNER_TLS_CLIENT_CA_PEM_FILE`
/// enables mTLS. Returns `None` when no cert/key is configured (loopback-only case).
fn load_tls() -> anyhow::Result<Option<TlsConfig>> {
	let (cert_file, key_file) = match (env::var("SIGNER_TLS_CERT_PEM_FILE").ok(), env::var("SIGNER_TLS_KEY_PEM_FILE").ok()) {
		(Some(cert), Some(key)) if !cert.is_empty() && !key.is_empty() => (cert, key),
		_ => return Ok(None),
	};
	let cert_pem = std::fs::read_to_string(&cert_file).with_context(|| format!("failed to read SIGNER_TLS_CERT_PEM_FILE at {cert_file}"))?;
	let key_pem = std::fs::read_to_string(&key_file).with_context(|| format!("failed to read SIGNER_TLS_KEY_PEM_FILE at {key_file}"))?;
	let client_ca_pem = match env::var("SIGNER_TLS_CLIENT_CA_PEM_FILE").ok().filter(|s| !s.is_empty()) {
		Some(ca_file) => Some(std::fs::read_to_string(&ca_file).with_context(|| format!("failed to read SIGNER_TLS_CLIENT_CA_PEM_FILE at {ca_file}"))?),
		None => None,
	};
	Ok(Some(TlsConfig { cert_pem, key_pem, client_ca_pem }))
}

fn split_csv(raw: &str) -> Vec<String> {
	raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn default_bind_is_loopback() {
		let addr: SocketAddr = DEFAULT_GRPC_ADDR.parse().expect("default addr parses");
		assert!(addr.ip().is_loopback(), "{DEFAULT_GRPC_ADDR} must default to loopback, not all interfaces");
	}
}

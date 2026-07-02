use tonic::{Code, Status};

use crate::key_vault::VaultError;

/// The signer's failure modes. `Crypto` deliberately carries NO detail across the
/// wire — a sealing/opening failure must not disclose whether it was a bad KEK, a
/// tampered blob, or an AAD mismatch.
#[derive(Debug, thiserror::Error)]
pub enum SignerError {
	#[error("vault error")]
	Crypto(#[from] VaultError),
	#[error("repository error: {0}")]
	Repository(String),
}

impl From<sqlx::Error> for SignerError {
	fn from(err: sqlx::Error) -> Self {
		SignerError::Repository(err.to_string())
	}
}

impl From<SignerError> for Status {
	/// Map to gRPC status without leaking internals: crypto/repository failures
	/// collapse to a generic `internal` (the secret-bearing detail never ships) —
	/// the real cause goes to the server log instead of the wire.
	fn from(err: SignerError) -> Self {
		match err {
			SignerError::Crypto(_) | SignerError::Repository(_) => {
				tracing::warn!(error = ?err, "signer error withheld from the wire");
				Status::new(Code::Internal, "internal error")
			}
		}
	}
}

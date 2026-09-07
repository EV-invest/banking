//! The key-backend seam — where a chain private key lives and how a digest gets signed.
//!
//! Everything above this module (the transaction builders, the gRPC handlers) only ever
//! sees a 32-byte digest going in and a [`ChainSignature`] coming out. Nothing above it
//! sees a private key. That is the whole point: signing is about to become an asynchronous
//! network call to a custodian that never releases the key, and the only way to get there
//! without touching the money-critical digest code is to put the seam here first.
//!
//! Today there is exactly one implementation, [`LocalVault`], which unseals the key from
//! `wallet_secrets` under the KEK and signs in-process — byte-for-byte what the signer did
//! before this module existed. A second implementation is what the migration adds.
//!
//! **The recovery id is always re-derived locally** ([`recover_id`]), never taken from
//! whatever produced the signature. A remote signer's `v` may be a raw 0/1 or a 27/28, and
//! guessing wrong yields a signature the node happily accepts but attributes to a
//! *different sender*. `LocalVault` goes through the same helper as any future backend, so
//! the canonical transaction vectors prove that path, not just a unit test.

use std::sync::Arc;

use domain::money::Network;
use ed25519_dalek::Signer as _;
use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use tonic::{Code, Status};
use uuid::Uuid;

use crate::{
	error::SignerError,
	key_vault::Vault,
	provision::{self, ProvisionedAddress},
	secrets::WalletSecrets,
};

/// The two signature curves the fund uses. BEP20, Polygon and TRC20 share secp256k1;
/// TON is Ed25519.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Curve {
	Secp256k1,
	Ed25519,
}

impl Curve {
	/// The curve a network's key is on — the single place that mapping is decided.
	pub fn of(network: Network) -> Self {
		match network {
			Network::Bep20 | Network::Trc20 | Network::Polygon => Curve::Secp256k1,
			Network::Ton => Curve::Ed25519,
		}
	}
}

/// A signature over a 32-byte digest, in the shape each chain's assembler needs.
///
/// `recovery_id` is the RAW 0/1 — the EIP-155 `recid + chain_id*2 + 35` offset and Tron's
/// 65th signature byte are the assemblers' business, not the backend's.
///
/// `Debug` is safe here and required by the `expect_err` assertions in `turnkey`: every field
/// is a PUBLIC component of a signature that goes on-chain verbatim. No secret is reachable
/// from this type — the private key never enters this process under the custodian backend, and
/// under the local one it lives in a `Zeroizing` buffer that is never placed in a signature.
#[derive(Debug)]
pub enum ChainSignature {
	Ecdsa { r: [u8; 32], s: [u8; 32], recovery_id: u8 },
	Ed25519([u8; 64]),
}

/// Which stored key signs. `(wallet_id, network)` is the `wallet_secrets` lookup key; the
/// backend resolves it to whatever it actually needs (a sealed blob here, a custodian's
/// account id later).
#[derive(Clone, Copy, Debug)]
pub struct KeyHandle {
	pub wallet_id: Uuid,
	pub network: Network,
}

/// A backend's failure modes, kept distinct because the hub's saga branches on them: a
/// terminal rejection fails a withdrawal, an `Unavailable` parks it for retry.
///
/// **The split is money-critical.** `custody.rs` maps `Unavailable`/`DeadlineExceeded` to
/// `CustodyError::Unavailable` (the outbox retries; nothing was sent) and EVERY other code
/// to `CustodyError::Rejected` (the withdrawal stops and waits for a human). A remote
/// backend's ten-second outage must therefore never reach the wire as anything but
/// `Unavailable` — see [`crate::turnkey`] for the classification that enforces it.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
	#[error("sending wallet is not provisioned")]
	NotProvisioned,
	/// The stored key cannot be turned into a usable signing key — under the local vault
	/// that means a KEK-epoch casualty: funds on that address cannot move.
	#[error("could not unseal the signing key")]
	KeyUnusable,
	/// The caller asked for a curve the handle's network does not use. A programming error
	/// (the wire guards in `service` already pin the rail), never a caller's fault.
	#[error("{network} keys are not on the requested curve")]
	CurveMismatch { network: Network },
	#[error("signing failed")]
	Signing,
	/// A remote key custodian could not be reached or could not answer *right now*: a
	/// timeout, a dropped connection, a 5xx, a rate limit. Nothing was signed and the same
	/// request will very likely succeed on the next attempt, so this — and only this —
	/// becomes `Status::unavailable` and lets the outbox retry.
	#[error("key custodian unavailable: {0}")]
	Unavailable(String),
	/// A remote key custodian refused ON THE MERITS: its policy engine said no, the account
	/// does not exist, the activity needs a human approval. Retrying reproduces the refusal,
	/// so this is terminal and the withdrawal parks for intervention.
	#[error("key custodian refused: {0}")]
	Rejected(String),
	/// The custodian answered something this code cannot interpret — a schema drift, a
	/// missing field, an activity result of the wrong shape. Our bug or the vendor's, never
	/// the caller's, and it will not heal by itself.
	#[error("key custodian protocol error: {0}")]
	Protocol(String),
	/// The row's `backend` column names a backend other than the one asked to sign it. Fails
	/// closed: signing a `backend='local'` row through the Turnkey client would ask Turnkey
	/// for a key it has never held. Phase 3 replaces this with per-row dispatch.
	#[error("wallet is stored under the {stored:?} backend, which is not the one serving this signer")]
	WrongBackend { stored: String },
	#[error(transparent)]
	Signer(#[from] SignerError),
}

impl From<BackendError> for Status {
	fn from(err: BackendError) -> Self {
		match err {
			BackendError::NotProvisioned => Status::failed_precondition("sending wallet is not provisioned"),
			BackendError::KeyUnusable => Status::new(Code::Internal, "could not unseal the signing key"),
			BackendError::CurveMismatch { .. } | BackendError::Signing => {
				tracing::warn!(error = ?err, "signing failed");
				Status::new(Code::Internal, "signing failed")
			}
			// The one retryable code. The message is a category we compose ourselves — never
			// a vendor response body, which can echo back the request.
			BackendError::Unavailable(ref reason) => {
				tracing::warn!(error = ?err, "key custodian unavailable — the hub will retry");
				Status::new(Code::Unavailable, format!("key custodian unavailable: {reason}"))
			}
			BackendError::Rejected(ref reason) => {
				tracing::error!(error = ?err, "key custodian refused on the merits — this withdrawal will NOT retry");
				Status::new(Code::PermissionDenied, format!("key custodian refused: {reason}"))
			}
			BackendError::Protocol(_) | BackendError::WrongBackend { .. } => {
				tracing::error!(error = ?err, "key backend protocol/composition failure");
				Status::new(Code::Internal, "signing failed")
			}
			BackendError::Signer(err) => err.into(),
		}
	}
}

/// The port every key backend implements. Async because the next implementation of it is a
/// network call; the transaction builders on either side of [`sign_digest`](Self::sign_digest)
/// stay synchronous and pure.
#[tonic::async_trait]
pub trait KeyBackend: Send + Sync {
	/// Provision (or return the existing) deposit address for `(user, network)`. Idempotent.
	async fn provision(&self, user_id: Uuid, network: Network) -> Result<ProvisionedAddress, BackendError>;

	/// The handle's public key — compressed SEC1 (33 bytes) on secp256k1, the raw 32-byte
	/// point on Ed25519. Needed *before* signing, not after: a Tron transaction bakes the
	/// owner address into the signed bytes and a TON external message is addressed to the
	/// wallet contract derived from this key. It is also what [`recover_id`] compares
	/// against, which is why a backend that never releases a private key must still expose
	/// this.
	async fn public_key(&self, handle: KeyHandle) -> Result<Vec<u8>, BackendError>;

	/// Sign a pre-computed 32-byte digest. The digest is final — a backend must never hash
	/// it again, or the signature covers something the chain never asked about.
	async fn sign_digest(&self, handle: KeyHandle, curve: Curve, digest: &[u8; 32]) -> Result<ChainSignature, BackendError>;
}

/// The in-process backend: keys sealed under the KEK in the signer's own `wallet_secrets`,
/// unsealed transiently for one signature.
pub struct LocalVault {
	vault: Arc<Vault>,
	secrets: WalletSecrets,
}

impl LocalVault {
	pub fn new(vault: Arc<Vault>, secrets: WalletSecrets) -> Self {
		Self { vault, secrets }
	}
}

#[tonic::async_trait]
impl KeyBackend for LocalVault {
	async fn provision(&self, user_id: Uuid, network: Network) -> Result<ProvisionedAddress, BackendError> {
		Ok(provision::provision(&self.vault, &self.secrets, user_id, network).await?)
	}

	async fn public_key(&self, handle: KeyHandle) -> Result<Vec<u8>, BackendError> {
		let (_, public_key) = self.secrets.find_watch(handle.wallet_id, handle.network).await?.ok_or(BackendError::NotProvisioned)?;
		Ok(public_key)
	}

	async fn sign_digest(&self, handle: KeyHandle, curve: Curve, digest: &[u8; 32]) -> Result<ChainSignature, BackendError> {
		if Curve::of(handle.network) != curve {
			return Err(BackendError::CurveMismatch { network: handle.network });
		}
		let KeyHandle { wallet_id, network } = handle;
		let sealed = self.secrets.find_sealed(wallet_id, network).await?.ok_or(BackendError::NotProvisioned)?;
		let opened = self.vault.open(provision::chain_of(network), &sealed.id.to_string(), &sealed.sealed_key).map_err(|err| {
			// ERROR, not WARN: an unopenable key at sign time means funds are already
			// stranded on its address (the KEK-epoch bug class). GetKeyHealth lists it;
			// RotateAddress restores the user's ability to receive future deposits.
			tracing::error!(error = %err, %wallet_id, %network, "could not unseal the signing key — PROVABLY DEAD KEY, funds on its address cannot move");
			BackendError::KeyUnusable
		})?;
		let secret = zeroize::Zeroizing::new(<[u8; 32]>::try_from(opened.as_slice()).map_err(|_| {
			tracing::warn!(len = opened.len(), %wallet_id, %network, "stored key is not 32 bytes");
			BackendError::KeyUnusable
		})?);
		sign_with_secret(curve, &secret, digest)
	}
}

/// Sign a digest with a plaintext 32-byte secret — the local backend's crypto core, and the
/// only place in the crate that still touches one.
///
/// The recovery id comes from [`recover_id`], not from `k256`'s own recoverable signer, so
/// the local and remote backends share one derivation. That costs a public-key recovery per
/// signature and buys a guarantee no vendor's `v` documentation can.
pub(crate) fn sign_with_secret(curve: Curve, secret: &[u8; 32], digest: &[u8; 32]) -> Result<ChainSignature, BackendError> {
	match curve {
		Curve::Secp256k1 => {
			let key = SigningKey::from_slice(secret).map_err(|_| BackendError::KeyUnusable)?;
			let signature: Signature = key.sign_prehash_recoverable(digest).map_err(|_| BackendError::Signing)?.0;
			let bytes = signature.to_bytes(); // 64 bytes, r || s, low-S normalized by k256
			let mut r = [0u8; 32];
			let mut s = [0u8; 32];
			r.copy_from_slice(&bytes[..32]);
			s.copy_from_slice(&bytes[32..]);
			let recovery_id = recover_id(&r, &s, digest, &key.verifying_key().to_sec1_bytes()).ok_or(BackendError::Signing)?;
			Ok(ChainSignature::Ecdsa { r, s, recovery_id })
		}
		Curve::Ed25519 => Ok(ChainSignature::Ed25519(ed25519_dalek::SigningKey::from_bytes(secret).sign(digest).to_bytes())),
	}
}

/// Recover the ECDSA recovery id for `(r, s)` over `digest`, given the public key that
/// signed it: try 0, try 1, keep the one whose recovered key is the known one.
///
/// This is the standard AWS-KMS-style derivation, and it is deliberately the ONLY way this
/// crate learns a recovery id — see the module docs for why a signer-supplied `v` is not
/// trusted. `None` when neither candidate recovers the expected key: a malformed signature,
/// a mismatched public key, or the ~2^-128 case where the true recovery id is 2 or 3 (`r`
/// wrapped the curve order). All three must fail closed — a wrong recovery id produces a
/// perfectly valid signature credited to somebody else's address.
pub fn recover_id(r: &[u8; 32], s: &[u8; 32], digest: &[u8; 32], public_key_sec1: &[u8]) -> Option<u8> {
	let expected = VerifyingKey::from_sec1_bytes(public_key_sec1).ok()?;
	let mut compact = [0u8; 64];
	compact[..32].copy_from_slice(r);
	compact[32..].copy_from_slice(s);
	let signature = Signature::from_slice(&compact).ok()?;
	(0u8..=1).find(|&candidate| {
		RecoveryId::from_byte(candidate)
			.and_then(|recid| VerifyingKey::recover_from_prehash(digest, &signature, recid).ok())
			.is_some_and(|recovered| recovered == expected)
	})
}

#[cfg(test)]
mod tests {
	use ed25519_dalek::{Verifier, VerifyingKey as Ed25519VerifyingKey};

	use super::*;
	use crate::key_vault::{ed25519_pubkey, gen_ed25519, gen_secp256k1, secp256k1_pubkey};

	/// The whole safety argument for not trusting a vendor's `v` rests on this: our own
	/// derivation must reproduce what k256 — which knows the recovery id for free, because
	/// it computed the signature — reports. Random keys and digests, so the two branches
	/// (recid 0 and 1) both get exercised many times over.
	#[test]
	fn recover_id_agrees_with_k256_own_recovery_id() {
		for _ in 0..200 {
			let secret = gen_secp256k1();
			let key = SigningKey::from_slice(&*secret).unwrap();
			let digest = *gen_ed25519(); // any 32 random bytes stand in for a chain digest

			let (signature, native): (Signature, RecoveryId) = key.sign_prehash_recoverable(&digest).unwrap();
			let bytes = signature.to_bytes();
			let (r, s): ([u8; 32], [u8; 32]) = (bytes[..32].try_into().unwrap(), bytes[32..].try_into().unwrap());

			let ours = recover_id(&r, &s, &digest, &key.verifying_key().to_sec1_bytes()).expect("a freshly made signature must recover");
			assert_eq!(ours, native.to_byte(), "locally derived recovery id must match k256's own");
		}
	}

	#[test]
	fn recover_id_refuses_a_foreign_public_key() {
		// A signature that verifies under one key must not yield a recovery id against
		// another — that is exactly the "valid signature, wrong sender" failure.
		let secret = gen_secp256k1();
		let key = SigningKey::from_slice(&*secret).unwrap();
		let digest = [9u8; 32];
		let bytes = key.sign_prehash_recoverable(&digest).unwrap().0.to_bytes();
		let (r, s): ([u8; 32], [u8; 32]) = (bytes[..32].try_into().unwrap(), bytes[32..].try_into().unwrap());

		assert!(recover_id(&r, &s, &digest, &key.verifying_key().to_sec1_bytes()).is_some());
		assert!(recover_id(&r, &s, &digest, &secp256k1_pubkey(&gen_secp256k1())).is_none());
		// A different digest under the same key is the same failure from the other side.
		assert!(recover_id(&r, &s, &[8u8; 32], &key.verifying_key().to_sec1_bytes()).is_none());
	}

	#[test]
	fn sign_with_secret_produces_a_verifiable_signature_on_both_curves() {
		let digest = [3u8; 32];

		let secp = gen_secp256k1();
		let ChainSignature::Ecdsa { r, s, recovery_id } = sign_with_secret(Curve::Secp256k1, &secp, &digest).unwrap() else {
			panic!("secp256k1 must yield an ECDSA signature");
		};
		assert!(recovery_id <= 1, "the raw recovery id is 0 or 1; the chain-specific offset is the assembler's job");
		assert_eq!(recover_id(&r, &s, &digest, &secp256k1_pubkey(&secp)), Some(recovery_id));

		let seed = gen_ed25519();
		let ChainSignature::Ed25519(signature) = sign_with_secret(Curve::Ed25519, &seed, &digest).unwrap() else {
			panic!("ed25519 must yield an Ed25519 signature");
		};
		let verifying = Ed25519VerifyingKey::from_bytes(&ed25519_pubkey(&seed)).unwrap();
		assert!(verifying.verify(&digest, &ed25519_dalek::Signature::from_bytes(&signature)).is_ok());
	}

	#[test]
	fn curve_of_pins_every_rail() {
		assert_eq!(Curve::of(Network::Bep20), Curve::Secp256k1);
		assert_eq!(Curve::of(Network::Polygon), Curve::Secp256k1);
		assert_eq!(Curve::of(Network::Trc20), Curve::Secp256k1);
		assert_eq!(Curve::of(Network::Ton), Curve::Ed25519);
	}
}

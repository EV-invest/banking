//! Minimal envelope-style key vault for an MVP custodial service.
//!
//! Stores chain private keys ENCRYPTED AT REST. Decryption happens only
//! transiently inside the signer; the plaintext is zeroized as soon as the
//! returned value is dropped.
//!
//! SCOPE / LIMIT (do not move this line):
//!   This protects a STOLEN DB or disk image. It does NOT protect against
//!   RCE on the running signer process (the key is in plaintext in memory at
//!   sign time). Use ONLY for a small hot-float you can afford to lose.
//!   Real balances belong behind MPC/HSM + an offline cold tier.
//!
//! Why XChaCha20-Poly1305: 24-byte nonce -> a random nonce per encryption is
//! safe, so there is zero nonce bookkeeping. That removes the classic footgun.
//!
//! Cargo.toml:
//!   chacha20poly1305 = "0.10"
//!   k256             = { version = "0.13", features = ["ecdsa"] }
//!   ed25519-dalek    = "2"
//!   zeroize          = "1"
//!   getrandom        = "0.2"
//!   hex              = "0.4"
//!   thiserror        = "1"
//!
//! Note: every random value here comes from `getrandom` (the OS CSPRNG)
//! directly, so the crates above do NOT need to agree on a `rand_core`
//! version. That avoids the most common Rust-crypto build headache.

// `chacha20poly1305 0.10` re-exports `generic-array 0.14`, whose `from_slice` is
// deprecated in favour of the 1.x API the crate hasn't upgraded to yet. The calls
// are correct for this pinned version; silence the dependency's deprecation notice
// without altering the crypto core.
#![allow(deprecated)]

use chacha20poly1305::{
	Key, XChaCha20Poly1305, XNonce,
	aead::{Aead, KeyInit, Payload},
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
	#[error("decrypt/encrypt failed (wrong KEK, tampered blob, or wrong chain/id)")]
	Crypto,
	#[error("KEK must be exactly 32 bytes (64 hex chars)")]
	KeyLen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chain {
	BscBep20,  // EVM, secp256k1
	TronTrc20, // secp256k1 (same curve as EVM)
	Ton,       // ed25519
}

impl Chain {
	fn tag(self) -> &'static [u8] {
		match self {
			Chain::BscBep20 => b"bsc",
			Chain::TronTrc20 => b"tron",
			Chain::Ton => b"ton",
		}
	}
}

/// The key-encrypting key (KEK).
///
/// It MUST be injected from OUTSIDE the app at runtime (secrets manager / KMS /
/// HSM-wrapped). It must NEVER live in the database, the repo, or a plaintext
/// config file next to the ciphertext. If the KEK and the encrypted blobs sit
/// in the same place, the encryption buys you almost nothing.
///
/// The KEK is wiped from memory on drop.
pub struct Vault {
	kek: Zeroizing<[u8; 32]>,
}
impl Vault {
	/// `kek_hex`: 64 hex chars (32 bytes). Read it from an env var that your
	/// host populates from a secrets store, e.g. `std::env::var("WALLET_KEK")`.
	pub fn from_hex(kek_hex: &str) -> Result<Self, VaultError> {
		let mut raw = hex::decode(kek_hex.trim()).map_err(|_| VaultError::KeyLen)?;
		if raw.len() != 32 {
			raw.zeroize();
			return Err(VaultError::KeyLen);
		}
		let mut k = [0u8; 32];
		k.copy_from_slice(&raw);
		raw.zeroize();
		Ok(Self { kek: Zeroizing::new(k) })
	}

	fn cipher(&self) -> XChaCha20Poly1305 {
		XChaCha20Poly1305::new(Key::from_slice(&*self.kek))
	}

	/// Encrypt a private key. Stored blob layout: `nonce(24) || ciphertext+tag`.
	/// `chain` + `wallet_id` are bound as AAD, so a blob cannot be silently
	/// moved onto another wallet's row.
	pub fn seal(&self, chain: Chain, wallet_id: &str, secret: &[u8]) -> Result<Vec<u8>, VaultError> {
		let nonce_bytes = random_bytes::<24>();
		let nonce = XNonce::from_slice(&nonce_bytes);
		let aad = aad(chain, wallet_id);
		let ct = self.cipher().encrypt(nonce, Payload { msg: secret, aad: &aad }).map_err(|_| VaultError::Crypto)?;
		let mut out = Vec::with_capacity(24 + ct.len());
		out.extend_from_slice(&nonce_bytes);
		out.extend_from_slice(&ct);
		Ok(out)
	}

	/// Decrypt. The plaintext is zeroized when the returned value is dropped,
	/// so: use it for the signature, then let it go out of scope. Do not log it.
	pub fn open(&self, chain: Chain, wallet_id: &str, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
		if blob.len() < 24 {
			return Err(VaultError::Crypto);
		}
		let (nonce_bytes, ct) = blob.split_at(24);
		let nonce = XNonce::from_slice(nonce_bytes);
		let aad = aad(chain, wallet_id);
		let pt = self.cipher().decrypt(nonce, Payload { msg: ct, aad: &aad }).map_err(|_| VaultError::Crypto)?;
		Ok(Zeroizing::new(pt))
	}
}

/// secp256k1 secret key. BSC/BEP20 and Tron/TRC20 share this curve.
pub fn gen_secp256k1() -> Zeroizing<[u8; 32]> {
	use k256::ecdsa::SigningKey;
	// Rejection sampling: a random 32-byte value exceeds secp256k1's order with
	// probability ~2^-128, so this all but always succeeds on the first iteration;
	// retrying is the correct, unbiased fix and there is no meaningful bound to enforce.
	//LOOP
	loop {
		let bytes = random_bytes::<32>();
		// Rejects the astronomically rare out-of-range scalar.
		if SigningKey::from_slice(&bytes).is_ok() {
			return Zeroizing::new(bytes);
		}
	}
}
/// ed25519 secret seed (TON). Any 32 bytes are a valid seed.
pub fn gen_ed25519() -> Zeroizing<[u8; 32]> {
	Zeroizing::new(random_bytes::<32>())
}
/// Compressed secp256k1 public key (33 bytes).
/// EVM address  = last 20 bytes of keccac256(uncompressed_pubkey[1..]).
/// Tron address = same 20 bytes, prefix 0x41, then Base58Check.
/// (use `alloy`/`ethers` for EVM and a tron lib for the encoding — no need
///  to hand-roll keccak/base58.)
pub fn secp256k1_pubkey(seed: &[u8; 32]) -> Vec<u8> {
	use k256::ecdsa::SigningKey;
	let sk = SigningKey::from_slice(seed).expect("valid secp256k1 scalar");
	sk.verifying_key().to_sec1_bytes().to_vec()
}
/// ed25519 public key (32 bytes). The TON wallet ADDRESS additionally needs a
/// wallet contract (v4/v5) + workchain; use a TON SDK to derive/deploy it.
pub fn ed25519_pubkey(seed: &[u8; 32]) -> [u8; 32] {
	use ed25519_dalek::SigningKey;
	SigningKey::from_bytes(seed).verifying_key().to_bytes()
}
/// 32 fresh bytes from the OS CSPRNG.
fn random_bytes<const N: usize>() -> [u8; N] {
	let mut b = [0u8; N];
	getrandom::getrandom(&mut b).expect("OS RNG unavailable");
	b
}

fn aad(chain: Chain, wallet_id: &str) -> Vec<u8> {
	let mut v = Vec::with_capacity(chain.tag().len() + 1 + wallet_id.len());
	v.extend_from_slice(chain.tag());
	v.push(b':');
	v.extend_from_slice(wallet_id.as_bytes());
	v
}

// --- per-chain key generation -------------------------------------------
// Generate the secret in the signer, immediately `seal` it, store the blob,
// and store the PUBLIC key/address for watch-only indexing.

// Suggested Postgres row:
//   id           uuid primary key
//   chain        text  not null         -- 'bsc' | 'tron' | 'ton'
//   public_key   bytea not null         -- for watch-only indexing
//   address      text  not null
//   enc_key      bytea not null         -- output of Vault::seal (nonce||ct)
//   created_at   timestamptz default now()

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn round_trip_and_tamper_resistance() {
		// 32-byte KEK as hex. In prod this comes from your secrets store.
		let kek_hex = hex::encode([7u8; 32]);
		let vault = Vault::from_hex(&kek_hex).unwrap();

		let sk = gen_secp256k1();
		let blob = vault.seal(Chain::BscBep20, "wallet-42", &*sk).unwrap();

		// correct chain + id -> recovers the exact secret
		let opened = vault.open(Chain::BscBep20, "wallet-42", &blob).unwrap();
		assert_eq!(&*opened, &sk[..]);

		// wrong wallet_id (AAD mismatch) -> fails, no plaintext leaks
		assert!(vault.open(Chain::BscBep20, "wallet-99", &blob).is_err());

		// wrong chain -> fails
		assert!(vault.open(Chain::TronTrc20, "wallet-42", &blob).is_err());

		// flipped byte -> auth tag rejects it
		let mut bad = blob.clone();
		*bad.last_mut().unwrap() ^= 0x01;
		assert!(vault.open(Chain::BscBep20, "wallet-42", &bad).is_err());

		// ed25519 path works too
		let ton = gen_ed25519();
		let ton_blob = vault.seal(Chain::Ton, "wallet-42", &*ton).unwrap();
		assert_eq!(&*vault.open(Chain::Ton, "wallet-42", &ton_blob).unwrap(), &ton[..]);
	}
}

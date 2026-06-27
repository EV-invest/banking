//! Key provisioning — the one use case the signer exposes today.
//!
//! Generate the curve keypair for a network, seal the private key at rest, persist
//! it, and hand back the public address. The private key is generated **in the
//! signer**, lives only in a [`Zeroizing`] buffer, is sealed immediately, and is
//! never logged — it leaves this function only as ciphertext.

use domain::money::{Network, WalletAddress};
use uuid::Uuid;

use crate::{
	error::SignerError,
	key_vault::{Chain, Vault, ed25519_pubkey, evm_address, gen_ed25519, gen_secp256k1, secp256k1_pubkey},
	secrets::{NewSecret, WalletSecrets},
};

/// Current sealing scheme. Bumped when the KEK or envelope format rotates, so an old
/// blob's `key_version` tells the signer how to open it. Only `1` exists today.
const KEY_VERSION: i32 = 1;

const BASE58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BASE64URL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Address-kind tags the signer reports to the hub. Real pubkey→address encoding is a
/// deferred feature, so today every address is a [`KIND_PLACEHOLDER`]; [`KIND_DERIVED`]
/// is the value the signer will report once it computes the true on-chain image. The hub
/// uses this to refuse to serve a placeholder as a fundable deposit destination.
pub const KIND_PLACEHOLDER: &str = "placeholder";
pub const KIND_DERIVED: &str = "derived";

/// A provisioned (or re-read) deposit address plus the [`KIND_PLACEHOLDER`]/
/// [`KIND_DERIVED`] tag the hub needs to decide whether the rail is fundable.
pub struct ProvisionedAddress {
	pub address: String,
	pub kind: &'static str,
}

/// Provision (or return the existing) key-backed deposit address for `(user, network)`.
/// Idempotent: a second call returns the first call's address without minting a new key.
pub async fn provision(vault: &Vault, secrets: &WalletSecrets, user_id: Uuid, network: Network) -> Result<ProvisionedAddress, SignerError> {
	// Existing key: re-derive the address from the stored public key (deterministic). For
	// BEP20 that is the real on-chain (derived) address; a previously-stored placeholder is
	// upgraded in place. Never opens the sealed private key.
	if let Some((stored, public_key)) = secrets.find_watch(user_id, network).await? {
		let (address, kind) = render_address(network, &public_key)?;
		if address != stored {
			secrets.update_address(user_id, network, &address).await?;
		}
		return Ok(ProvisionedAddress { address, kind });
	}

	let generated = generate(network);
	let id = Uuid::new_v4();
	let (address, _) = render_address(network, &generated.pubkey)?;
	// AAD binds the blob to this chain + this row id, so it can't be replayed onto
	// another wallet's row. `id` is the row PK → also the future signing path's lookup.
	let sealed = vault.seal(chain_of(network), &id.to_string(), &*generated.secret)?;

	secrets
		.insert(&NewSecret {
			id,
			user_id,
			network,
			public_key: &generated.pubkey,
			address: address.as_str(),
			sealed_key: &sealed,
			key_alg: generated.alg,
			key_version: KEY_VERSION,
		})
		.await?;

	// Re-read the canonical row: ours, or a concurrent racer's whose insert won (ours was
	// then a no-op and its sealed key was dropped/zeroized unused). Derive from its public
	// key so the returned address always matches the persisted row.
	let (stored, public_key) = secrets
		.find_watch(user_id, network)
		.await?
		.ok_or_else(|| SignerError::Repository("wallet_secrets row missing immediately after insert".into()))?;
	let (address, kind) = render_address(network, &public_key)?;
	if address != stored {
		secrets.update_address(user_id, network, &address).await?;
	}
	Ok(ProvisionedAddress { address, kind })
}

/// The on-chain address for a stored/fresh public key, plus its kind. **BEP20** derives the
/// real EVM address (EIP-55) and reports [`KIND_DERIVED`] — fundable. TRC20/TON still return
/// a [`KIND_PLACEHOLDER`] until their encodings land (Base58Check / the TON wallet contract).
fn render_address(network: Network, public_key: &[u8]) -> Result<(String, &'static str), SignerError> {
	match network {
		Network::Bep20 => {
			let address = evm_address(public_key).ok_or_else(|| SignerError::Repository("EVM address derivation failed for a stored secp256k1 key".into()))?;
			Ok((address, KIND_DERIVED))
		}
		Network::Trc20 | Network::Ton => Ok((placeholder_address(network, public_key)?.as_str().to_owned(), KIND_PLACEHOLDER)),
	}
}

struct Generated {
	alg: &'static str,
	/// 32-byte private scalar/seed, zeroized on drop.
	secret: zeroize::Zeroizing<[u8; 32]>,
	pubkey: Vec<u8>,
}

fn generate(network: Network) -> Generated {
	match network {
		Network::Ton => {
			let secret = gen_ed25519();
			let pubkey = ed25519_pubkey(&secret).to_vec();
			Generated { alg: "ed25519", secret, pubkey }
		}
		// BEP20 and TRC20 share secp256k1.
		Network::Bep20 | Network::Trc20 => {
			let secret = gen_secp256k1();
			let pubkey = secp256k1_pubkey(&secret);
			Generated { alg: "secp256k1", secret, pubkey }
		}
	}
}

pub fn chain_of(network: Network) -> Chain {
	match network {
		Network::Bep20 => Chain::BscBep20,
		Network::Trc20 => Chain::TronTrc20,
		Network::Ton => Chain::Ton,
	}
}

/// A structurally-valid address **bound to the real public key** — but NOT yet its
/// cryptographic image. Real pubkey→address encoding (EVM keccak, Tron Base58Check,
/// TON wallet-contract v4/v5 via a TON SDK) is a separate feature; until it lands the
/// signer returns this placeholder so the watch-only surface keeps a stable, parseable
/// address per wallet. Recompute from the stored `public_key` when real encoding ships.
fn placeholder_address(network: Network, pubkey: &[u8]) -> Result<WalletAddress, SignerError> {
	let byte_at = |i: usize| pubkey[i % pubkey.len()];
	let rendered = match network {
		// EVM: 0x + 40 hex (20 bytes).
		Network::Bep20 => {
			let mut s = String::from("0x");
			for i in 0..20 {
				s.push_str(&format!("{:02x}", byte_at(i)));
			}
			s
		}
		// TRON: 'T' + 33 base58 chars.
		Network::Trc20 => {
			let mut s = String::from("T");
			for i in 0..33 {
				s.push(BASE58[byte_at(i) as usize % BASE58.len()] as char);
			}
			s
		}
		// TON: 48-char user-friendly base64url.
		Network::Ton => {
			let mut s = String::with_capacity(48);
			for i in 0..48 {
				s.push(BASE64URL[byte_at(i) as usize % BASE64URL.len()] as char);
			}
			s
		}
	};
	WalletAddress::parse(network, &rendered).map_err(|e| SignerError::Repository(e.to_string()))
}

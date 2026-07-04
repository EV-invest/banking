//! Key provisioning — the one use case the signer exposes today.
//!
//! Generate the curve keypair for a network, seal the private key at rest, persist
//! it, and hand back the public address. The private key is generated **in the
//! signer**, lives only in a [`Zeroizing`] buffer, is sealed immediately, and is
//! never logged — it leaves this function only as ciphertext.

use domain::money::Network;
use uuid::Uuid;

use crate::{
	error::SignerError,
	key_vault::{Chain, Vault, ed25519_pubkey, evm_address, gen_ed25519, gen_secp256k1, secp256k1_pubkey, ton_address, tron_address},
	secrets::{NewSecret, WalletSecrets},
};

/// Current sealing scheme. Bumped when the KEK or envelope format rotates, so an old
/// blob's `key_version` tells the signer how to open it. Only `1` exists today.
const KEY_VERSION: i32 = 1;

/// Address-kind tag the signer reports to the hub. Every supported rail now computes its
/// true on-chain image, so provisioning reports [`KIND_DERIVED`]; the hub still gates on
/// this to refuse serving a non-derived address as a fundable deposit destination.
pub const KIND_DERIVED: &str = "derived";

/// A provisioned (or re-read) deposit address plus the [`KIND_DERIVED`] tag the hub needs
/// to decide whether the rail is fundable.
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

	// Unseal-probe BEFORE the address can leave the signer: prove the blob opens under
	// the current KEK and recovers the exact secret. A key that cannot be opened would
	// otherwise surface only when a sweep/withdrawal tries to sign — with real funds
	// already stranded on the address (the KEK-epoch incident this guards against).
	let reopened = vault.open(chain_of(network), &id.to_string(), &sealed)?;
	if reopened.as_slice() != &generated.secret[..] {
		return Err(SignerError::Repository("unseal probe recovered different bytes than were sealed — refusing to provision".into()));
	}
	drop(reopened);

	let kek_fp = vault.fingerprint();
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
			kek_fp: &kek_fp,
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

/// The on-chain address for a stored/fresh public key, plus its kind. **BEP20** derives the real
/// EVM address (EIP-55), **TRC20** the real Base58Check `T…` address, and **TON** the real v4R2
/// wallet (StateInit hash) — all [`KIND_DERIVED`] (fundable).
fn render_address(network: Network, public_key: &[u8]) -> Result<(String, &'static str), SignerError> {
	match network {
		Network::Bep20 => {
			let address = evm_address(public_key).ok_or_else(|| SignerError::Repository("EVM address derivation failed for a stored secp256k1 key".into()))?;
			Ok((address, KIND_DERIVED))
		}
		Network::Trc20 => {
			let address = tron_address(public_key).ok_or_else(|| SignerError::Repository("Tron address derivation failed for a stored secp256k1 key".into()))?;
			Ok((address, KIND_DERIVED))
		}
		Network::Ton => {
			let address = ton_address(public_key).ok_or_else(|| SignerError::Repository("TON address derivation failed for a stored ed25519 key".into()))?;
			Ok((address, KIND_DERIVED))
		}
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

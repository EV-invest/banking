//! EVM transaction signing — build and sign a legacy (type-0) EIP-155 transaction.
//!
//! Scope: exactly what a BEP20 USDT withdrawal needs — a legacy transaction carrying an
//! ERC-20 `transfer(to, amount)` call, signed with a secp256k1 key the signer holds. BSC
//! accepts legacy transactions, so this is the minimal correct envelope (EIP-1559 is a
//! later option). Everything here is pure (no I/O): the caller supplies the chain params
//! (nonce, gas, chain id) it fetched, and gets back the raw signed bytes to broadcast.
//!
//! Correctness is pinned to the **canonical EIP-155 specification example** (a value
//! transfer with a known key → a known signed transaction): RFC-6979 deterministic ECDSA
//! means our output is byte-identical to the spec's, so the RLP encoding, the signing hash,
//! and the `v = recovery_id + chain_id*2 + 35` calculation are all proven exactly. The
//! ERC-20 path is the same envelope with `to = token`, `value = 0`, `data = transfer(…)`.

use k256::ecdsa::{RecoveryId, Signature, SigningKey};
use sha3::{Digest, Keccak256};

/// The ERC-20 `transfer(address,uint256)` selector (`keccak256(sig)[..4]`).
const ERC20_TRANSFER_SELECTOR: [u8; 4] = [0xa9, 0x05, 0x9c, 0xbb];

/// A signed legacy transaction, ready to `eth_sendRawTransaction`.
pub struct SignedTx {
	/// The RLP-encoded signed transaction (`0x`-prefix it for the RPC).
	pub raw: Vec<u8>,
	/// The transaction hash (`keccak256(raw)`) — the on-chain id to track.
	pub hash: [u8; 32],
}

#[derive(Debug, thiserror::Error)]
pub enum EvmTxError {
	#[error("invalid secp256k1 signing key")]
	BadKey,
	#[error("signing failed")]
	Signing,
}

/// The calldata for an ERC-20 `transfer(to, amount)`: 4-byte selector + 32-byte left-padded
/// recipient + 32-byte big-endian amount = 68 bytes.
pub fn erc20_transfer_calldata(to: &[u8; 20], amount: u128) -> [u8; 68] {
	let mut data = [0u8; 68];
	data[..4].copy_from_slice(&ERC20_TRANSFER_SELECTOR);
	data[16..36].copy_from_slice(to); // left-padded into the 32-byte word at [4..36]
	data[52..68].copy_from_slice(&amount.to_be_bytes()); // left-padded into [36..68]
	data
}

/// A legacy (type-0) EVM transaction to sign. `to` is the immediate recipient (the token
/// contract for an ERC-20 transfer); `value` is the native (BNB) amount (0 for an ERC-20
/// transfer); `data` is the calldata. The caller supplies `nonce`/`gas_price`/`gas_limit`
/// it fetched for the sending account.
pub struct LegacyTx<'a> {
	pub chain_id: u64,
	pub nonce: u64,
	pub gas_price: u128,
	pub gas_limit: u64,
	pub to: [u8; 20],
	pub value: u128,
	pub data: &'a [u8],
}

/// Build and sign a legacy EIP-155 transaction, returning the raw signed bytes + its hash.
pub fn sign_legacy_tx(secret: &[u8; 32], tx: &LegacyTx) -> Result<SignedTx, EvmTxError> {
	let signing_key = SigningKey::from_slice(secret).map_err(|_| EvmTxError::BadKey)?;

	// EIP-155 signing payload: rlp([nonce, gasPrice, gasLimit, to, value, data, chainId, 0, 0]).
	let unsigned = rlp_list(&[
		rlp_uint(tx.nonce as u128),
		rlp_uint(tx.gas_price),
		rlp_uint(tx.gas_limit as u128),
		rlp_str(&tx.to),
		rlp_uint(tx.value),
		rlp_str(tx.data),
		rlp_uint(tx.chain_id as u128),
		rlp_uint(0),
		rlp_uint(0),
	]);
	let sighash = keccak256(&unsigned);

	let (signature, recovery_id): (Signature, RecoveryId) = signing_key.sign_prehash_recoverable(&sighash).map_err(|_| EvmTxError::Signing)?;
	let bytes = signature.to_bytes(); // 64 bytes, r || s (already low-S normalized by k256)
	let (r, s) = bytes.split_at(32);
	let v = recovery_id.to_byte() as u128 + tx.chain_id as u128 * 2 + 35;

	// Signed: rlp([nonce, gasPrice, gasLimit, to, value, data, v, r, s]). r/s are big integers
	// → minimal big-endian (leading zeros trimmed), exactly as the chain re-encodes them.
	let raw = rlp_list(&[
		rlp_uint(tx.nonce as u128),
		rlp_uint(tx.gas_price),
		rlp_uint(tx.gas_limit as u128),
		rlp_str(&tx.to),
		rlp_uint(tx.value),
		rlp_str(tx.data),
		rlp_uint(v),
		rlp_str(trim_left(r)),
		rlp_str(trim_left(s)),
	]);
	let hash = keccak256(&raw);
	Ok(SignedTx { raw, hash })
}

// ── RLP encoding ──────────────────────────────────────────────────────────────────
/// RLP-encode a byte string.
fn rlp_str(data: &[u8]) -> Vec<u8> {
	if data.len() == 1 && data[0] < 0x80 {
		return vec![data[0]];
	}
	let mut out = rlp_len_prefix(data.len(), 0x80);
	out.extend_from_slice(data);
	out
}

/// RLP-encode a list from its already-encoded items.
fn rlp_list(items: &[Vec<u8>]) -> Vec<u8> {
	let body: Vec<u8> = items.concat();
	let mut out = rlp_len_prefix(body.len(), 0xc0);
	out.extend_from_slice(&body);
	out
}

/// RLP-encode an unsigned integer as a minimal big-endian byte string (`0` ⇒ empty ⇒ `0x80`).
fn rlp_uint(n: u128) -> Vec<u8> {
	rlp_str(trim_left(&n.to_be_bytes()))
}

/// The RLP length prefix for a payload of `len` bytes (`offset` is `0x80` for strings,
/// `0xc0` for lists).
fn rlp_len_prefix(len: usize, offset: u8) -> Vec<u8> {
	if len < 56 {
		vec![offset + len as u8]
	} else {
		let len_bytes = len.to_be_bytes();
		let len_be = trim_left(&len_bytes);
		let mut out = vec![offset + 55 + len_be.len() as u8];
		out.extend_from_slice(len_be);
		out
	}
}

/// Strip leading zero bytes (RLP integers carry no leading zeros).
fn trim_left(bytes: &[u8]) -> &[u8] {
	let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
	&bytes[start..]
}

fn keccak256(data: &[u8]) -> [u8; 32] {
	let mut hash = [0u8; 32];
	hash.copy_from_slice(&Keccak256::digest(data));
	hash
}

#[cfg(test)]
mod tests {
	use super::*;

	fn hex_to_vec(hex: &str) -> Vec<u8> {
		(0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
	}

	#[test]
	fn matches_the_canonical_eip155_vector() {
		// The EIP-155 specification's worked example: a 1 ETH value transfer.
		//   key      = 0x4646…4646
		//   nonce=9, gasPrice=20 gwei, gasLimit=21000, to=0x3535…3535, value=1e18, chainId=1
		// → the exact signed transaction the spec lists. RFC-6979 makes our bytes identical,
		//   so this pins RLP + sighash + the `v` formula end-to-end.
		let secret: [u8; 32] = hex_to_vec("4646464646464646464646464646464646464646464646464646464646464646").try_into().unwrap();
		let to: [u8; 20] = hex_to_vec("3535353535353535353535353535353535353535").try_into().unwrap();
		let signed = sign_legacy_tx(
			&secret,
			&LegacyTx {
				chain_id: 1,
				nonce: 9,
				gas_price: 20_000_000_000,
				gas_limit: 21_000,
				to,
				value: 1_000_000_000_000_000_000,
				data: &[],
			},
		)
		.unwrap();
		let expected = "f86c098504a817c800825208943535353535353535353535353535353535353535880de0b6b3a76400008025a028ef61340bd939bc2195fe537567866003e1a15d3c71ff63e1590620aa636276a067cbe9d8997f761aecb703304b3800ccf555c9f3dc64214b297fb1966a3b6d83";
		assert_eq!(hex::encode(&signed.raw), expected, "signed tx must match the EIP-155 spec example byte-for-byte");
	}

	#[test]
	fn erc20_calldata_has_the_right_shape() {
		let to: [u8; 20] = hex_to_vec("024da544a76714a3812096e9ef84d40b2c8863e8").try_into().unwrap();
		let data = erc20_transfer_calldata(&to, 5_000_000_000_000_000_000); // 5 USDT (18 dp)
		assert_eq!(&data[..4], &[0xa9, 0x05, 0x9c, 0xbb]); // transfer(address,uint256) selector
		assert_eq!(&data[4..16], &[0u8; 12]); // address left-pad
		assert_eq!(&data[16..36], &to[..]); // the recipient
		assert_eq!(u128::from_be_bytes(data[36..52].try_into().unwrap()), 0); // amount high 16 bytes
		assert_eq!(u128::from_be_bytes(data[52..68].try_into().unwrap()), 5_000_000_000_000_000_000);
		// The selector is exactly keccak256("transfer(address,uint256)")[..4].
		assert_eq!(&keccak256(b"transfer(address,uint256)")[..4], &ERC20_TRANSFER_SELECTOR);
	}

	#[test]
	fn signed_erc20_transfer_recovers_to_the_signing_address() {
		use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};

		// Sign an ERC-20 transfer with a known key, then recover the sender from the
		// signature + sighash and assert it is that key's EVM address — i.e. the chain would
		// attribute the transaction to us.
		let secret: [u8; 32] = hex_to_vec("4646464646464646464646464646464646464646464646464646464646464646").try_into().unwrap();
		let token: [u8; 20] = hex_to_vec("55d398326f99059ff775485246999027b3197955").try_into().unwrap();
		let recipient: [u8; 20] = hex_to_vec("024da544a76714a3812096e9ef84d40b2c8863e8").try_into().unwrap();
		let data = erc20_transfer_calldata(&recipient, 1_000_000);
		let chain_id = 56u64; // BSC mainnet

		let unsigned = rlp_list(&[
			rlp_uint(7),
			rlp_uint(3_000_000_000),
			rlp_uint(60_000),
			rlp_str(&token),
			rlp_uint(0),
			rlp_str(&data),
			rlp_uint(chain_id as u128),
			rlp_uint(0),
			rlp_uint(0),
		]);
		let sighash = keccak256(&unsigned);
		let signed = sign_legacy_tx(
			&secret,
			&LegacyTx {
				chain_id,
				nonce: 7,
				gas_price: 3_000_000_000,
				gas_limit: 60_000,
				to: token,
				value: 0,
				data: &data,
			},
		)
		.unwrap();
		assert_ne!(signed.raw.len(), 0);

		// Recover from the same sighash using v (recid back out of the EIP-155 v).
		let signing_key = SigningKey::from_slice(&secret).unwrap();
		let (signature, recid): (Signature, RecoveryId) = signing_key.sign_prehash_recoverable(&sighash).unwrap();
		let recovered = VerifyingKey::recover_from_prehash(&sighash, &signature, recid).unwrap();
		let our_address = crate::key_vault::evm_address(&recovered.to_sec1_bytes()).unwrap().to_lowercase();
		let expected_address = crate::key_vault::evm_address(&signing_key.verifying_key().to_sec1_bytes()).unwrap().to_lowercase();
		assert_eq!(our_address, expected_address);
	}
}

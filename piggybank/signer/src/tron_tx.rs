//! Tron transaction signing — build and sign a protobuf transaction (not RLP).
//!
//! Scope: exactly what a TRC20 USDT withdrawal/sweep needs — a `transfer(to, amount)` via a
//! `TriggerSmartContract`, plus a native TRX `TransferContract` for gas-station top-ups.
//! Everything is pure (no I/O): the caller supplies the recent-block reference + window it fetched
//! (`getnowblock`), and gets back the raw signed transaction bytes + the txID to persist and
//! broadcast — the same division of labour as the EVM path passing the nonce/gas it fetched.
//!
//! Divergence from EVM (see `evm_tx`): a Tron tx is protobuf, the signed digest is
//! `sha256(serialize(Transaction.raw))` (the txID), and the signature is a plain 65-byte
//! recoverable `r || s || recovery_id` where `recovery_id` is the raw 0/1 — NOT EIP-155's
//! `recid + chain_id*2 + 35`. There is no nonce: replay protection is the ref-block + the ~60s
//! `expiration` + the unique txID, so the caller persists the signed bytes before broadcasting and
//! only ever re-signs once the prior tx has provably expired without landing.

#[cfg(test)]
use k256::ecdsa::{RecoveryId, Signature};
use prost::Message;
use sha2::{Digest, Sha256};

use crate::{backend::ChainSignature, evm_tx::erc20_transfer_calldata, key_vault};

mod proto {
	#![allow(clippy::all, clippy::pedantic, missing_docs)]
	include!(concat!(env!("OUT_DIR"), "/protocol.rs"));
}

use proto::transaction::contract::ContractType;

/// `Any.type_url`s the node dispatches on — must match the vendored message names exactly.
const TRIGGER_SMART_CONTRACT_TYPE_URL: &str = "type.googleapis.com/protocol.TriggerSmartContract";
const TRANSFER_CONTRACT_TYPE_URL: &str = "type.googleapis.com/protocol.TransferContract";

/// A signed Tron transaction, ready for `/wallet/broadcasthex`.
pub struct SignedTronTx {
	/// Hex of the full signed `Transaction` protobuf.
	pub raw_tx: String,
	/// Hex of the txID — `sha256(serialize(Transaction.raw))`, the on-chain id + idempotency key.
	pub txid: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TronTxError {
	#[error("invalid secp256k1 signing key")]
	BadKey,
	#[error("a Tron transaction needs an ECDSA signature")]
	WrongSignatureKind,
	#[error("amount exceeds the protocol's int64 range")]
	Amount,
}

/// The recent-block reference + window every Tron transaction carries (fetched by the hub from
/// `getnowblock`), the analogue of the EVM nonce/gas the caller supplies.
pub struct TxRef {
	/// Low 2 bytes of the reference block height.
	pub ref_block_bytes: Vec<u8>,
	/// Bytes [8,16) of the reference block id.
	pub ref_block_hash: Vec<u8>,
	/// Unix-ms after which the tx can never be included (ref head ts + ~60s).
	pub expiration: i64,
	/// Unix-ms the tx was built.
	pub timestamp: i64,
}

/// A built-but-unsigned Tron transaction: the protobuf `Transaction.raw` whose serialization
/// the txID (and therefore the signature) covers. Opaque — mutating it after the digest was
/// taken would silently invalidate the signature.
pub struct UnsignedTronTx {
	raw: proto::transaction::Raw,
}

/// Build a TRC20 `transfer(recipient, amount)` and return the txID to sign. `owner` is the
/// 21-byte raw Tron address of the SIGNING key — the node rejects any transaction whose
/// `owner_address` does not match the signature, so it must be the signer's own. `token` and
/// `recipient` are 21-byte raw Tron addresses (`0x41 || account`); `amount` is 6-dp USDT base
/// units.
pub fn build_unsigned_trc20(owner: &[u8; 21], token: &[u8; 21], recipient: &[u8; 21], amount: u128, fee_limit: i64, tx_ref: &TxRef) -> Result<(UnsignedTronTx, [u8; 32]), TronTxError> {
	let mut to = [0u8; 20];
	to.copy_from_slice(&recipient[1..]); // the ABI address arg is the 20-byte body, no 0x41
	let data = erc20_transfer_calldata(&to, amount);
	let trigger = proto::TriggerSmartContract {
		owner_address: owner.to_vec(),
		contract_address: token.to_vec(),
		call_value: 0,
		data: data.to_vec(),
		call_token_value: 0,
		token_id: 0,
	};
	let contract = contract(ContractType::TriggerSmartContract, TRIGGER_SMART_CONTRACT_TYPE_URL, trigger.encode_to_vec());
	Ok(digest(raw(tx_ref, contract, fee_limit)))
}

/// Build a native TRX transfer (a gas-station top-up) and return the txID to sign. `owner` is
/// the signing key's own 21-byte raw address, `to` a 21-byte raw address, `amount` SUN.
pub fn build_unsigned_trx(owner: &[u8; 21], to: &[u8; 21], amount: u128, tx_ref: &TxRef) -> Result<(UnsignedTronTx, [u8; 32]), TronTxError> {
	let transfer = proto::TransferContract {
		owner_address: owner.to_vec(),
		to_address: to.to_vec(),
		amount: i64::try_from(amount).map_err(|_| TronTxError::Amount)?,
	};
	let contract = contract(ContractType::TransferContract, TRANSFER_CONTRACT_TYPE_URL, transfer.encode_to_vec());
	// A native transfer is bandwidth-only — no fee_limit (0 ⇒ omitted on the wire).
	Ok(digest(raw(tx_ref, contract, 0)))
}

fn contract(kind: ContractType, type_url: &str, value: Vec<u8>) -> proto::transaction::Contract {
	proto::transaction::Contract {
		r#type: kind as i32,
		parameter: Some(proto::Any {
			type_url: type_url.to_owned(),
			value,
		}),
		provider: Vec::new(),
		contract_name: Vec::new(),
		permission_id: 0,
	}
}

fn raw(tx_ref: &TxRef, contract: proto::transaction::Contract, fee_limit: i64) -> proto::transaction::Raw {
	proto::transaction::Raw {
		ref_block_bytes: tx_ref.ref_block_bytes.clone(),
		ref_block_num: 0,
		ref_block_hash: tx_ref.ref_block_hash.clone(),
		expiration: tx_ref.expiration,
		data: Vec::new(),
		contract: vec![contract],
		timestamp: tx_ref.timestamp,
		fee_limit,
	}
}

/// txID = `sha256(serialize(raw))` — the digest a Tron signature covers, and the on-chain id.
fn digest(raw: proto::transaction::Raw) -> (UnsignedTronTx, [u8; 32]) {
	let txid: [u8; 32] = Sha256::digest(raw.encode_to_vec()).into();
	(UnsignedTronTx { raw }, txid)
}

/// Finish the transaction with a signature over the txID: pack `r || s || recovery_id` (the
/// RAW 0/1 — Tron has no EIP-155 offset) and wrap into the full `Transaction`. The owner is
/// already baked into `raw`, so this must be the signature of that owner's key.
pub fn assemble(parts: UnsignedTronTx, signature: &ChainSignature) -> Result<SignedTronTx, TronTxError> {
	let ChainSignature::Ecdsa { r, s, recovery_id } = signature else {
		return Err(TronTxError::WrongSignatureKind);
	};
	let mut sig = Vec::with_capacity(65);
	sig.extend_from_slice(r);
	sig.extend_from_slice(s);
	sig.push(*recovery_id);

	let txid = Sha256::digest(parts.raw.encode_to_vec());
	let tx = proto::Transaction {
		raw_data: Some(parts.raw),
		signature: vec![sig],
	};
	Ok(SignedTronTx {
		raw_tx: hex::encode(tx.encode_to_vec()),
		txid: hex::encode(txid),
	})
}

/// The 21-byte raw Tron address that must own the transaction, from the signing key's own
/// compressed secp256k1 public key. `owner_address` has to match the signature or the node
/// rejects the transaction, so the caller derives it from the very key it will sign with.
pub fn owner_address(public_key: &[u8]) -> Result<[u8; 21], TronTxError> {
	key_vault::tron_raw_address(public_key).ok_or(TronTxError::BadKey)
}

/// The pre-seam one-shot paths, kept for the tests below ONLY — production signs through
/// [`crate::backend::KeyBackend`]. They run the exact production pieces
/// (`build_unsigned_*` → the backend's signing core → `assemble`) so the existing
/// recover-to-the-owner vectors keep proving the split unchanged.
#[cfg(test)]
fn sign_trc20_transfer(secret: &[u8; 32], token: &[u8; 21], recipient: &[u8; 21], amount: u128, fee_limit: i64, tx_ref: &TxRef) -> Result<SignedTronTx, TronTxError> {
	let owner = owner_address(&key_vault::secp256k1_pubkey(secret))?;
	let (parts, txid) = build_unsigned_trc20(&owner, token, recipient, amount, fee_limit, tx_ref)?;
	assemble(parts, &test_sign(secret, &txid))
}

#[cfg(test)]
fn sign_trx_transfer(secret: &[u8; 32], to: &[u8; 21], amount: u128, tx_ref: &TxRef) -> Result<SignedTronTx, TronTxError> {
	let owner = owner_address(&key_vault::secp256k1_pubkey(secret))?;
	let (parts, txid) = build_unsigned_trx(&owner, to, amount, tx_ref)?;
	assemble(parts, &test_sign(secret, &txid))
}

#[cfg(test)]
fn test_sign(secret: &[u8; 32], digest: &[u8; 32]) -> ChainSignature {
	crate::backend::sign_with_secret(crate::backend::Curve::Secp256k1, secret, digest).expect("the test key signs")
}

#[cfg(test)]
mod tests {
	use k256::ecdsa::VerifyingKey;

	use super::*;

	fn ref_for_test() -> TxRef {
		TxRef {
			ref_block_bytes: vec![0x12, 0x34],
			ref_block_hash: vec![1, 2, 3, 4, 5, 6, 7, 8],
			expiration: 1_700_000_060_000,
			timestamp: 1_700_000_000_000,
		}
	}

	#[test]
	fn signed_trc20_transfer_recovers_to_the_owner() {
		// Sign a USDT transfer with privkey=1 (Tron address TMVQGm…HK2HC), then recover the signer
		// from the signature over the txID and assert it is that address — i.e. the chain would
		// attribute the transaction to us. Also re-derive the txID from the serialized raw_data,
		// proving txID = sha256(raw_data) and the protobuf round-trips. (A Nile testnet broadcast
		// is the final external proof; this pins the envelope + signature without a node.)
		let mut secret = [0u8; 32];
		secret[31] = 1;
		let token = key_vault::tron_base58_to_raw("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t").unwrap();
		let recipient = key_vault::tron_base58_to_raw("TJRabPrwbZy45sbavfcjinPJC18kjpRTv8").unwrap();
		let signed = sign_trc20_transfer(&secret, &token, &recipient, 1_000_000, 100_000_000, &ref_for_test()).unwrap();

		let tx = proto::Transaction::decode(hex::decode(&signed.raw_tx).unwrap().as_slice()).unwrap();
		let raw = tx.raw_data.clone().unwrap();
		assert_eq!(hex::encode(Sha256::digest(raw.encode_to_vec())), signed.txid);

		let sig = &tx.signature[0];
		let signature = Signature::from_slice(&sig[..64]).unwrap();
		let recid = RecoveryId::from_byte(sig[64]).unwrap();
		let txid = hex::decode(&signed.txid).unwrap();
		let recovered = VerifyingKey::recover_from_prehash(&txid, &signature, recid).unwrap();
		assert_eq!(key_vault::tron_address(&recovered.to_sec1_bytes()).unwrap(), "TMVQGm1qAQYVdetCeGRRkTWYYrLXuHK2HC");

		let contract = &raw.contract[0];
		assert_eq!(contract.r#type, ContractType::TriggerSmartContract as i32);
		// The calldata is the shared ERC-20 transfer selector + the recipient's 20-byte body.
		let trigger = proto::TriggerSmartContract::decode(contract.parameter.as_ref().unwrap().value.as_slice()).unwrap();
		assert_eq!(&trigger.data[..4], &[0xa9, 0x05, 0x9c, 0xbb]);
		assert_eq!(&trigger.data[16..36], &recipient[1..]);
		assert_eq!(trigger.contract_address, token.to_vec());
	}

	#[test]
	fn signed_trx_transfer_is_a_native_transfer_from_the_owner() {
		let mut secret = [0u8; 32];
		secret[31] = 1;
		let to = key_vault::tron_base58_to_raw("TJRabPrwbZy45sbavfcjinPJC18kjpRTv8").unwrap();
		let signed = sign_trx_transfer(&secret, &to, 30_000_000, &ref_for_test()).unwrap();

		let tx = proto::Transaction::decode(hex::decode(&signed.raw_tx).unwrap().as_slice()).unwrap();
		let raw = tx.raw_data.unwrap();
		let contract = &raw.contract[0];
		assert_eq!(contract.r#type, ContractType::TransferContract as i32);
		let transfer = proto::TransferContract::decode(contract.parameter.as_ref().unwrap().value.as_slice()).unwrap();
		assert_eq!(transfer.amount, 30_000_000);
		assert_eq!(transfer.to_address, to.to_vec());
		assert_eq!(key_vault::tron_address(&key_vault::secp256k1_pubkey(&secret)).unwrap(), "TMVQGm1qAQYVdetCeGRRkTWYYrLXuHK2HC");
		assert_eq!(transfer.owner_address, key_vault::tron_raw_address(&key_vault::secp256k1_pubkey(&secret)).unwrap().to_vec());
	}
}

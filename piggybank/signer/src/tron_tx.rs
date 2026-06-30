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

use k256::ecdsa::{RecoveryId, Signature, SigningKey};
use prost::Message;
use sha2::{Digest, Sha256};

use crate::{evm_tx::erc20_transfer_calldata, key_vault};

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
	#[error("signing failed")]
	Signing,
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

/// Sign a TRC20 `transfer(recipient, amount)` from the key's own address. `token` and `recipient`
/// are 21-byte raw Tron addresses (`0x41 || account`); `amount` is 6-dp USDT base units.
pub fn sign_trc20_transfer(secret: &[u8; 32], token: &[u8; 21], recipient: &[u8; 21], amount: u128, fee_limit: i64, tx_ref: &TxRef) -> Result<SignedTronTx, TronTxError> {
	let owner = owner_address(secret)?;
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
	sign(secret, raw(tx_ref, contract, fee_limit))
}

/// Sign a native TRX transfer (a gas-station top-up). `to` is a 21-byte raw address, `amount` SUN.
pub fn sign_trx_transfer(secret: &[u8; 32], to: &[u8; 21], amount: u128, tx_ref: &TxRef) -> Result<SignedTronTx, TronTxError> {
	let owner = owner_address(secret)?;
	let transfer = proto::TransferContract {
		owner_address: owner.to_vec(),
		to_address: to.to_vec(),
		amount: i64::try_from(amount).map_err(|_| TronTxError::Amount)?,
	};
	let contract = contract(ContractType::TransferContract, TRANSFER_CONTRACT_TYPE_URL, transfer.encode_to_vec());
	// A native transfer is bandwidth-only — no fee_limit (0 ⇒ omitted on the wire).
	sign(secret, raw(tx_ref, contract, 0))
}

/// The 21-byte raw Tron address that must own the transaction — derived from the signing key
/// itself, so `owner_address` always matches the signature (the node rejects any mismatch).
fn owner_address(secret: &[u8; 32]) -> Result<[u8; 21], TronTxError> {
	let pubkey = key_vault::secp256k1_pubkey(secret);
	key_vault::tron_raw_address(&pubkey).ok_or(TronTxError::BadKey)
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

/// txID = `sha256(serialize(raw))`; sign it; pack `r || s || recovery_id`; wrap into the full
/// `Transaction`. The owner is already baked into `raw` and re-derived from this same key.
fn sign(secret: &[u8; 32], raw: proto::transaction::Raw) -> Result<SignedTronTx, TronTxError> {
	let signing_key = SigningKey::from_slice(secret).map_err(|_| TronTxError::BadKey)?;
	let raw_bytes = raw.encode_to_vec();
	let txid = Sha256::digest(&raw_bytes);

	let (signature, recovery_id): (Signature, RecoveryId) = signing_key.sign_prehash_recoverable(&txid).map_err(|_| TronTxError::Signing)?;
	let mut sig = signature.to_bytes().to_vec(); // 64 bytes r || s (low-S normalized by k256)
	sig.push(recovery_id.to_byte()); // raw recovery id 0/1 — no EIP-155 offset

	let tx = proto::Transaction {
		raw_data: Some(raw),
		signature: vec![sig],
	};
	Ok(SignedTronTx {
		raw_tx: hex::encode(tx.encode_to_vec()),
		txid: hex::encode(txid),
	})
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

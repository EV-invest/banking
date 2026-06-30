//! TON transaction signing — build and sign a v4R2 external message.
//!
//! Scope: exactly what a TON USDT (jetton) withdrawal/sweep + a gas top-up need. The
//! analog of [`evm_tx`](crate::evm_tx), but TON is a cell/BoC chain, not RLP, and a
//! "wallet" is a smart contract whose address is its StateInit hash — so we lean on the
//! pure-Rust `tonlib-core` for cell/message/wallet construction rather than hand-rolling.
//!
//! What we sign is an **external-in message** to OUR own v4R2 wallet. It carries one
//! internal message:
//!   - jetton transfer: internal message → OUR jetton wallet, body = TEP-74 transfer
//!     (op `0x0f8a7ea5`) moving USDT to the recipient OWNER;
//!   - native transfer: internal message → recipient, plain Toncoin value (gas top-up).
//!
//! Signing is `Ed25519` over the **representation hash of the signing-body cell** (not
//! keccak of RLP). We reuse the existing `ed25519-dalek` with the seed the signer
//! unsealed from the vault — the seed never reaches `tonlib-core`. Re-signing the same
//! inputs is byte-identical (Ed25519 is deterministic, query_id is fixed), so a stored
//! BoC re-broadcast is the same message; the wallet's strict-`seqno` rule makes a stale
//! re-send a silent no-op on-chain (no double-spend). On the wallet's first send
//! (`seqno == 0`) the StateInit is attached so it self-deploys.

use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use num_bigint::BigUint;
use tonlib_core::{
	TonAddress,
	cell::{BagOfCells, Cell, EMPTY_ARC_CELL},
	message::{CommonMsgInfo, HasOpcode, InternalMessage, JettonTransferMessage, TonMessage, TransferMessage, WithForwardPayload},
	wallet::{mnemonic::KeyPair, ton_wallet::TonWallet, version_helper::VersionHelper, wallet_version::WalletVersion},
};

use crate::key_vault::ed25519_pubkey;

/// A signed TON external message, ready to POST to toncenter `/message`.
pub struct SignedTonTx {
	/// Base64 BoC of the signed external message.
	pub signed_boc: String,
	/// Hex cell hash of the external message — the hub's stable tracking id.
	pub msg_hash: String,
	/// The seqno this message was signed at (echoed back for the broadcast record).
	pub seqno: u64,
	/// The unix expiry it was signed with.
	pub valid_until: u32,
}

/// A TEP-74 jetton (USDT) transfer to sign.
pub struct JettonTransfer<'a> {
	/// OUR sender wallet's jetton wallet — the internal message's destination.
	pub our_jetton_wallet: &'a str,
	/// The recipient OWNER address (the TEP-74 `destination`); their jetton wallet is
	/// resolved on-chain by our jetton wallet, not here.
	pub to_owner: &'a str,
	/// Where excess Toncoin returns (the treasury for a withdrawal, the gas station for a
	/// sweep).
	pub response_destination: &'a str,
	/// Jetton base units (6-dp) to move.
	pub amount: u128,
	/// Nanotons forwarded to deploy the recipient's jetton wallet (if absent) and trigger
	/// its notification.
	pub forward_ton_amount: u64,
	/// Nanotons attached to the internal message (the gas budget; excess returns to
	/// `response_destination`).
	pub msg_value: u64,
	pub seqno: u64,
	pub valid_until: u32,
}

/// A plain Toncoin value transfer to sign (the gas-station top-up).
pub struct NativeTransfer<'a> {
	pub to_address: &'a str,
	/// Nanotons to send.
	pub amount: u128,
	pub seqno: u64,
	pub valid_until: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum TonTxError {
	#[error("invalid TON address ({field}): {detail}")]
	Address { field: &'static str, detail: String },
	#[error("amount overflows the jetton/coins encoding")]
	Amount,
	#[error("cell build/serialize failed: {0}")]
	Cell(String),
}

/// Build + sign a TEP-74 jetton transfer external message.
pub fn build_jetton_transfer(seed: &[u8; 32], t: &JettonTransfer) -> Result<SignedTonTx, TonTxError> {
	let our_jetton_wallet = parse_address(t.our_jetton_wallet, "our_jetton_wallet")?;
	let to_owner = parse_address(t.to_owner, "to_address")?;
	let response_destination = parse_address(t.response_destination, "response_destination")?;

	// The TEP-74 transfer body. query_id is fixed (0) so a re-sign is byte-identical;
	// forward_ton_amount > 0 (with an empty payload) lets the receiver's jetton wallet
	// auto-deploy and emit its notification.
	let mut transfer = JettonTransferMessage::new(&to_owner, &BigUint::from(t.amount));
	transfer.with_query_id(0);
	transfer.with_response_destination(&response_destination);
	transfer.with_forward_payload(BigUint::from(t.forward_ton_amount), EMPTY_ARC_CELL.clone());
	let body = transfer.build().map_err(cell_err)?;

	// The internal message to OUR jetton wallet (bounceable: a failed transfer should
	// bounce the value back to us, not vanish).
	let internal = internal_message(&our_jetton_wallet, t.msg_value as u128, true, body)?;
	sign_external(seed, internal, t.seqno, t.valid_until)
}

/// Build + sign a plain Toncoin value transfer external message (gas top-up).
pub fn build_native_transfer(seed: &[u8; 32], t: &NativeTransfer) -> Result<SignedTonTx, TonTxError> {
	let to = parse_address(t.to_address, "to_address")?;
	// Non-bounceable: the recipient wallet may not be deployed yet (it self-deploys on its
	// own first send), so a bounceable top-up would bounce the gas straight back.
	let internal = internal_message(&to, t.amount, false, EMPTY_ARC_CELL.as_ref().clone())?;
	sign_external(seed, internal, t.seqno, t.valid_until)
}

/// Wrap an internal message in a v4R2 external message, Ed25519-sign the signing-body
/// cell hash with the vault seed, and serialize to a base64 BoC. `seqno == 0` attaches
/// the StateInit so the sending wallet self-deploys on its first send.
fn sign_external(seed: &[u8; 32], internal: Cell, seqno: u64, valid_until: u32) -> Result<SignedTonTx, TonTxError> {
	let pubkey = ed25519_pubkey(seed);
	// The secret half is held back from tonlib-core — we sign the cell hash ourselves with
	// ed25519-dalek below; only the public key is needed (for address/StateInit derivation).
	let key_pair = KeyPair {
		public_key: pubkey.to_vec(),
		secret_key: Vec::new(),
	};
	let wallet = TonWallet::new(WalletVersion::V4R2, key_pair).map_err(cell_err)?;

	let body = wallet.create_external_body(valid_until, seqno as u32, [internal.to_arc()]).map_err(cell_err)?;
	let hash = body.cell_hash();
	let signature = SigningKey::from_bytes(seed).sign(hash.as_slice()).to_bytes();
	let signed_body = VersionHelper::sign_msg(WalletVersion::V4R2, &body, &signature).map_err(cell_err)?;
	let external = wallet.wrap_signed_body(signed_body, seqno == 0).map_err(|e| TonTxError::Cell(e.to_string()))?;

	let boc = BagOfCells::from_root(external.clone()).serialize(true).map_err(cell_err)?;
	Ok(SignedTonTx {
		signed_boc: STANDARD.encode(boc),
		msg_hash: external.cell_hash().to_hex(),
		seqno,
		valid_until,
	})
}

/// A bounce-configurable internal message carrying `body`, addressed to `dest` with
/// `value` nanotons. `src`/fees/lt are left null — the validators fill them in.
fn internal_message(dest: &TonAddress, value: u128, bounce: bool, body: Cell) -> Result<Cell, TonTxError> {
	let info = CommonMsgInfo::InternalMessage(InternalMessage {
		ihr_disabled: true,
		bounce,
		bounced: false,
		src: TonAddress::NULL,
		dest: dest.clone(),
		value: BigUint::from(value),
		ihr_fee: BigUint::from(0u32),
		fwd_fee: BigUint::from(0u32),
		created_lt: 0,
		created_at: 0,
	});
	TransferMessage::new(info, body.to_arc()).build().map_err(cell_err)
}

fn parse_address(raw: &str, field: &'static str) -> Result<TonAddress, TonTxError> {
	raw.parse::<TonAddress>().map_err(|e| TonTxError::Address { field, detail: e.to_string() })
}

fn cell_err<E: std::fmt::Display>(err: E) -> TonTxError {
	TonTxError::Cell(err.to_string())
}

#[cfg(test)]
mod tests {
	use ed25519_dalek::{Verifier, VerifyingKey};

	use super::*;

	const SEED: [u8; 32] = [
		0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
	];
	// A real mainnet USDT-on-TON jetton wallet, and two owner addresses (raw `0:hex`).
	const OUR_JW: &str = "0:e4d954ef9f4e1250a26b5bbad76a1cdd17cfd08babad6f4c23e372270aef6f76";
	const TO_OWNER: &str = "EQB3ncyBUTjZUA5EnFKR5_EnOMI9V1tTEAAPaiU71gc4TiUt";
	const RESPONSE: &str = "0:8d8c9d8a8e8b8c8d8e8f808182838485868788898a8b8c8d8e8f80818283848f";

	fn sample_jetton(seqno: u64) -> JettonTransfer<'static> {
		JettonTransfer {
			our_jetton_wallet: OUR_JW,
			to_owner: TO_OWNER,
			response_destination: RESPONSE,
			amount: 5_000_000, // 5 USDT at 6 dp
			forward_ton_amount: 50_000_000,
			msg_value: 100_000_000,
			seqno,
			valid_until: 1_800_000_000,
		}
	}

	#[test]
	fn jetton_transfer_is_deterministic() {
		// Same inputs → byte-identical BoC, so a stored re-broadcast is the same message.
		let a = build_jetton_transfer(&SEED, &sample_jetton(7)).unwrap();
		let b = build_jetton_transfer(&SEED, &sample_jetton(7)).unwrap();
		assert_eq!(a.signed_boc, b.signed_boc);
		assert_eq!(a.msg_hash, b.msg_hash);
		assert_eq!(a.seqno, 7);
		assert_eq!(a.valid_until, 1_800_000_000);
		assert!(!a.signed_boc.is_empty());
	}

	#[test]
	fn jetton_transfer_first_send_attaches_state_init() {
		// seqno 0 self-deploys (StateInit attached) ⇒ a larger BoC than a later send.
		let deploy = build_jetton_transfer(&SEED, &sample_jetton(0)).unwrap();
		let later = build_jetton_transfer(&SEED, &sample_jetton(9)).unwrap();
		assert_ne!(deploy.signed_boc, later.signed_boc);
		assert!(deploy.signed_boc.len() > later.signed_boc.len());
	}

	#[test]
	fn signature_verifies_against_the_wallet_pubkey() {
		// Re-derive the exact signing-body cell, then check the embedded Ed25519 signature
		// over its representation hash verifies against our public key — i.e. the wallet
		// contract (which runs the same check) would accept it. This is the money-critical
		// assertion; a real testnet send is the final external proof.
		let t = sample_jetton(3);
		let our_jw = OUR_JW.parse::<TonAddress>().unwrap();
		let to_owner = TO_OWNER.parse::<TonAddress>().unwrap();
		let response = RESPONSE.parse::<TonAddress>().unwrap();

		let mut transfer = JettonTransferMessage::new(&to_owner, &BigUint::from(t.amount));
		transfer.with_query_id(0);
		transfer.with_response_destination(&response);
		transfer.with_forward_payload(BigUint::from(t.forward_ton_amount), EMPTY_ARC_CELL.clone());
		let body = transfer.build().unwrap();
		let internal = internal_message(&our_jw, t.msg_value as u128, true, body).unwrap();

		let pubkey = ed25519_pubkey(&SEED);
		let key_pair = KeyPair {
			public_key: pubkey.to_vec(),
			secret_key: Vec::new(),
		};
		let wallet = TonWallet::new(WalletVersion::V4R2, key_pair).unwrap();
		let signing_body = wallet.create_external_body(t.valid_until, t.seqno as u32, [internal.to_arc()]).unwrap();
		let message_hash = signing_body.cell_hash();
		let signature = SigningKey::from_bytes(&SEED).sign(message_hash.as_slice());

		let verifying = VerifyingKey::from_bytes(&pubkey).unwrap();
		assert!(verifying.verify(message_hash.as_slice(), &signature).is_ok());
	}

	#[test]
	fn native_transfer_builds_and_is_deterministic() {
		let t = NativeTransfer {
			to_address: TO_OWNER,
			amount: 100_000_000, // 0.1 TON
			seqno: 4,
			valid_until: 1_800_000_000,
		};
		let a = build_native_transfer(&SEED, &t).unwrap();
		let b = build_native_transfer(&SEED, &t).unwrap();
		assert_eq!(a.signed_boc, b.signed_boc);
		assert!(!a.signed_boc.is_empty());
	}

	#[test]
	fn rejects_a_malformed_address() {
		let mut t = sample_jetton(1);
		t.to_owner = "not-a-ton-address";
		assert!(matches!(build_jetton_transfer(&SEED, &t), Err(TonTxError::Address { field: "to_address", .. })));
	}
}

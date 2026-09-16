//! The fee budget is enforced by every signing handler, on every wallet class, before any
//! key is touched. Real Postgres, the real local vault, the real handlers (no mocks).
//!
//! Two observations per rail:
//!   - an over-budget quote from a wallet that was NEVER provisioned is refused with
//!     `PermissionDenied` — not `FailedPrecondition` ("not provisioned") — which proves the
//!     gate runs before the backend is consulted at all, let alone asked to sign;
//!   - a quote exactly at the default ceiling from a provisioned deposit wallet (not the
//!     treasury: this is not a treasury control) is signed.
//!
//! Runs when `SIGNER_DATABASE_URL`/`DATABASE_URL` is set; skips otherwise.

mod common;

use domain::money::Network;
use evbanking_contracts::signer::v1::{SignErc20TransferRequest, SignJettonTransferRequest, SignNativeTransferRequest, SignTrc20TransferRequest, signer_service_server::SignerService};
use piggybank_signer::{key_vault::Vault, policy::SignerPolicy, provision, secrets::WalletSecrets, service::Signer};
use tonic::{Code, Request};
use uuid::Uuid;

const GWEI: u128 = 1_000_000_000;

fn test_vault() -> Vault {
	Vault::from_hex(&hex::encode([9u8; 32])).unwrap()
}

async fn signer_and_wallet(db: &common::TestDb, network: Network) -> (Signer, Uuid) {
	let secrets = WalletSecrets::new(db.pool.clone());
	let user = Uuid::new_v4();
	provision::provision(&test_vault(), &secrets, user, network).await.expect("provision a deposit wallet");
	(Signer::new(test_vault(), secrets, SignerPolicy::default()), user)
}

fn erc20(from: Uuid, gas_price: u128, gas_limit: u64) -> Request<SignErc20TransferRequest> {
	Request::new(SignErc20TransferRequest {
		from_user_id: from.to_string(),
		network: "bep20".to_owned(),
		token_contract: "0x55d398326f99059ff775485246999027b3197955".to_owned(),
		to_address: "0x024da544a76714a3812096e9ef84d40b2c8863e8".to_owned(),
		amount: "1".to_owned(),
		chain_id: 56,
		nonce: 0,
		gas_price: gas_price.to_string(),
		gas_limit,
	})
}

fn native(from: Uuid, gas_price: u128, gas_limit: u64) -> Request<SignNativeTransferRequest> {
	Request::new(SignNativeTransferRequest {
		from_user_id: from.to_string(),
		network: "polygon".to_owned(),
		to_address: "0x024da544a76714a3812096e9ef84d40b2c8863e8".to_owned(),
		amount: "1".to_owned(),
		chain_id: 137,
		nonce: 0,
		gas_price: gas_price.to_string(),
		gas_limit,
	})
}

fn trc20(from: Uuid, fee_limit: i64) -> Request<SignTrc20TransferRequest> {
	Request::new(SignTrc20TransferRequest {
		from_user_id: from.to_string(),
		network: "trc20".to_owned(),
		token_contract: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".to_owned(),
		to_address: "TJRabPrwbZy45sbavfcjinPJC18kjpRTv8".to_owned(),
		amount: "1".to_owned(),
		ref_block_bytes: "0102".to_owned(),
		ref_block_hash: "0102030405060708".to_owned(),
		expiration: 1_800_000_060_000,
		timestamp: 1_800_000_000_000,
		fee_limit,
	})
}

fn jetton(from: Uuid, msg_value: u64, forward_ton_amount: u64) -> Request<SignJettonTransferRequest> {
	Request::new(SignJettonTransferRequest {
		from_user_id: from.to_string(),
		network: "ton".to_owned(),
		our_jetton_wallet: "0:e4d954ef9f4e1250a26b5bbad76a1cdd17cfd08babad6f4c23e372270aef6f76".to_owned(),
		to_address: "EQB3ncyBUTjZUA5EnFKR5_EnOMI9V1tTEAAPaiU71gc4TiUt".to_owned(),
		amount: "1".to_owned(),
		response_destination: "0:8d8c9d8a8e8b8c8d8e8f808182838485868788898a8b8c8d8e8f80818283848f".to_owned(),
		forward_ton_amount,
		msg_value,
		seqno: 0,
		valid_until: 1_800_000_000,
		is_testnet: false,
		wallet_version: String::new(),
	})
}

#[tokio::test]
async fn erc20_transfer_is_bounded_by_the_evm_budget() {
	let Some(db) = common::throwaway_db().await else {
		eprintln!("DATABASE_URL/SIGNER_DATABASE_URL unset — skipping signer fee budget test");
		return;
	};
	let (signer, wallet) = signer_and_wallet(&db, Network::Bep20).await;

	let refused = signer.sign_erc20_transfer(erc20(Uuid::new_v4(), 100 * GWEI + 1, 100_000)).await.unwrap_err();
	assert_eq!(refused.code(), Code::PermissionDenied, "{refused:?}");
	assert!(refused.message().contains("gas_price"), "{refused:?}");

	signer.sign_erc20_transfer(erc20(wallet, 100 * GWEI, 100_000)).await.expect("at the ceiling is signed");
	db.cleanup().await;
}

#[tokio::test]
async fn native_transfer_is_bounded_by_the_evm_budget() {
	let Some(db) = common::throwaway_db().await else {
		eprintln!("DATABASE_URL/SIGNER_DATABASE_URL unset — skipping signer fee budget test");
		return;
	};
	let (signer, wallet) = signer_and_wallet(&db, Network::Polygon).await;

	let refused = signer.sign_native_transfer(native(Uuid::new_v4(), 1, 100_001)).await.unwrap_err();
	assert_eq!(refused.code(), Code::PermissionDenied, "{refused:?}");
	assert!(refused.message().contains("gas_limit"), "{refused:?}");

	// Polygon's own ceiling, not BSC's: 5_000 gwei is signed here.
	signer.sign_native_transfer(native(wallet, 5_000 * GWEI, 21_000)).await.expect("at the ceiling is signed");
	db.cleanup().await;
}

#[tokio::test]
async fn trc20_transfer_is_bounded_by_the_tron_budget() {
	let Some(db) = common::throwaway_db().await else {
		eprintln!("DATABASE_URL/SIGNER_DATABASE_URL unset — skipping signer fee budget test");
		return;
	};
	let (signer, wallet) = signer_and_wallet(&db, Network::Trc20).await;

	let refused = signer.sign_trc20_transfer(trc20(Uuid::new_v4(), 100_000_001)).await.unwrap_err();
	assert_eq!(refused.code(), Code::PermissionDenied, "{refused:?}");
	assert!(refused.message().contains("fee_limit"), "{refused:?}");

	signer.sign_trc20_transfer(trc20(wallet, 100_000_000)).await.expect("at the ceiling is signed");
	db.cleanup().await;
}

#[tokio::test]
async fn jetton_transfer_is_bounded_by_the_ton_budget() {
	let Some(db) = common::throwaway_db().await else {
		eprintln!("DATABASE_URL/SIGNER_DATABASE_URL unset — skipping signer fee budget test");
		return;
	};
	let (signer, wallet) = signer_and_wallet(&db, Network::Ton).await;

	let refused = signer.sign_jetton_transfer(jetton(Uuid::new_v4(), 100_000_000, 50_000_001)).await.unwrap_err();
	assert_eq!(refused.code(), Code::PermissionDenied, "{refused:?}");
	assert!(refused.message().contains("forward_ton_amount"), "{refused:?}");

	signer.sign_jetton_transfer(jetton(wallet, 100_000_000, 50_000_000)).await.expect("at the ceiling is signed");
	db.cleanup().await;
}

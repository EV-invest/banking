//! The gRPC driving adapter — the thin hub↔signer seam.
//!
//! It validates the wire request, unseals the relevant key transiently, and delegates the
//! key handling to the modules below it ([`provision`], [`evm_tx`], [`key_vault`]). A
//! plaintext key never leaves a handler. `Result<_, Status>` is tonic's mandated handler
//! signature and `Status` is a large type we don't control.
#![allow(clippy::result_large_err)]

use domain::money::Network;
use evbanking_contracts::signer::v1::{
	ProvisionAddressRequest, ProvisionAddressResponse, SignErc20TransferRequest, SignErc20TransferResponse, SignJettonTransferRequest, SignNativeTransferRequest, SignNativeTransferResponse,
	SignTonTransferRequest, SignTrc20TransferRequest, SignTrxTransferRequest, SignedTonTxResponse, SignedTronTxResponse, signer_service_server::SignerService,
};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{evm_tx, key_vault::Vault, policy::SignerPolicy, provision, secrets::WalletSecrets, ton_tx, tron_tx};

/// The reserved wallet id for the treasury hot wallet. Real user ids are random v4 UUIDs
/// (never nil), so the treasury shares the `(user_id, network)` store without a schema
/// change. A withdrawal sends from here; a sweep sends INTO here from user addresses.
const TREASURY_WALLET: Uuid = Uuid::nil();

/// The signer service: the loaded [`Vault`] (holding the KEK), the `wallet_secrets` store,
/// and the independent spend [`SignerPolicy`] (the second gate — cap/allowlist).
pub struct Signer {
	vault: Vault,
	secrets: WalletSecrets,
	policy: SignerPolicy,
}

impl Signer {
	pub fn new(vault: Vault, secrets: WalletSecrets, policy: SignerPolicy) -> Self {
		Self { vault, secrets, policy }
	}

	/// Apply the spend policy to a USDT transfer signed FROM the treasury (the drain vector).
	/// Transfers from any other wallet — a user's deposit address being swept in, the gas
	/// station — are not treasury spends and pass unchecked.
	fn guard_treasury_transfer(&self, wallet_id: Uuid, network: Network, to_address: &str, amount_base_units: u128) -> Result<(), Status> {
		if wallet_id == TREASURY_WALLET {
			self.policy.check_treasury_transfer(network, to_address, amount_base_units)?;
		}
		Ok(())
	}

	/// Resolve the sending wallet id from the wire `from_user_id`: empty ⇒ the treasury hot
	/// wallet (nil), else a parsed UUID (a real user's deposit address, or the gas station).
	fn resolve_wallet(from_user_id: &str) -> Result<Uuid, Status> {
		if from_user_id.is_empty() {
			Ok(TREASURY_WALLET)
		} else {
			Uuid::parse_str(from_user_id).map_err(|_| Status::invalid_argument("from_user_id must be a UUID or empty"))
		}
	}

	/// Transiently unseal a sending wallet's secp256k1 key into a `Zeroizing` buffer (wiped
	/// on drop). The plaintext key exists only for the duration of one signing call and never
	/// leaves this process.
	async fn unseal(&self, wallet_id: Uuid, network: Network) -> Result<zeroize::Zeroizing<[u8; 32]>, Status> {
		let sealed = self
			.secrets
			.find_sealed(wallet_id, network)
			.await?
			.ok_or_else(|| Status::failed_precondition("sending wallet is not provisioned"))?;
		let opened = self
			.vault
			.open(provision::chain_of(network), &sealed.id.to_string(), &sealed.sealed_key)
			.map_err(|_| Status::internal("could not unseal the signing key"))?;
		Ok(zeroize::Zeroizing::new(
			<[u8; 32]>::try_from(opened.as_slice()).map_err(|_| Status::internal("stored key is not 32 bytes"))?,
		))
	}
}

#[tonic::async_trait]
impl SignerService for Signer {
	async fn provision_address(&self, request: Request<ProvisionAddressRequest>) -> Result<Response<ProvisionAddressResponse>, Status> {
		let req = request.into_inner();
		let user_id = Uuid::parse_str(&req.user_id).map_err(|_| Status::invalid_argument("user_id must be a UUID"))?;
		let network = Network::parse(&req.network).map_err(|_| Status::invalid_argument(format!("unknown network: {}", req.network)))?;
		// The kind tag makes the signer fail closed: it never claims a placeholder is a
		// fundable address — it labels it, and the hub refuses to serve it as one.
		let provisioned = provision::provision(&self.vault, &self.secrets, user_id, network).await?;
		Ok(Response::new(ProvisionAddressResponse {
			address: provisioned.address,
			address_kind: provisioned.kind.to_owned(),
		}))
	}

	async fn sign_erc20_transfer(&self, request: Request<SignErc20TransferRequest>) -> Result<Response<SignErc20TransferResponse>, Status> {
		let req = request.into_inner();
		let network = Network::parse(&req.network).map_err(|_| Status::invalid_argument(format!("unknown network: {}", req.network)))?;
		let wallet_id = Self::resolve_wallet(&req.from_user_id)?;
		let token = parse_evm_address(&req.token_contract).ok_or_else(|| Status::invalid_argument("token_contract must be a 0x 20-byte address"))?;
		let to = parse_evm_address(&req.to_address).ok_or_else(|| Status::invalid_argument("to_address must be a 0x 20-byte address"))?;
		let amount: u128 = req.amount.parse().map_err(|_| Status::invalid_argument("amount must be a u128 decimal"))?;
		let gas_price: u128 = req.gas_price.parse().map_err(|_| Status::invalid_argument("gas_price must be a u128 decimal"))?;
		self.guard_treasury_transfer(wallet_id, network, &req.to_address, amount)?;

		let secret = self.unseal(wallet_id, network).await?;
		let data = evm_tx::erc20_transfer_calldata(&to, amount);
		let signed = evm_tx::sign_legacy_tx(
			&secret,
			&evm_tx::LegacyTx {
				chain_id: req.chain_id,
				nonce: req.nonce,
				gas_price,
				gas_limit: req.gas_limit,
				to: token,
				value: 0,
				data: &data,
			},
		)
		.map_err(|_| Status::internal("signing failed"))?;

		Ok(Response::new(SignErc20TransferResponse {
			raw_tx: format!("0x{}", hex::encode(&signed.raw)),
			tx_hash: format!("0x{}", hex::encode(signed.hash)),
		}))
	}

	async fn sign_native_transfer(&self, request: Request<SignNativeTransferRequest>) -> Result<Response<SignNativeTransferResponse>, Status> {
		let req = request.into_inner();
		let network = Network::parse(&req.network).map_err(|_| Status::invalid_argument(format!("unknown network: {}", req.network)))?;
		let wallet_id = Self::resolve_wallet(&req.from_user_id)?;
		let to = parse_evm_address(&req.to_address).ok_or_else(|| Status::invalid_argument("to_address must be a 0x 20-byte address"))?;
		let amount: u128 = req.amount.parse().map_err(|_| Status::invalid_argument("amount must be a u128 decimal"))?;
		let gas_price: u128 = req.gas_price.parse().map_err(|_| Status::invalid_argument("gas_price must be a u128 decimal"))?;

		let secret = self.unseal(wallet_id, network).await?;
		// A native transfer carries the value directly and no calldata.
		let signed = evm_tx::sign_legacy_tx(
			&secret,
			&evm_tx::LegacyTx {
				chain_id: req.chain_id,
				nonce: req.nonce,
				gas_price,
				gas_limit: req.gas_limit,
				to,
				value: amount,
				data: &[],
			},
		)
		.map_err(|_| Status::internal("signing failed"))?;

		Ok(Response::new(SignNativeTransferResponse {
			raw_tx: format!("0x{}", hex::encode(&signed.raw)),
			tx_hash: format!("0x{}", hex::encode(signed.hash)),
		}))
	}

	async fn sign_trc20_transfer(&self, request: Request<SignTrc20TransferRequest>) -> Result<Response<SignedTronTxResponse>, Status> {
		let req = request.into_inner();
		let network = Network::parse(&req.network).map_err(|_| Status::invalid_argument(format!("unknown network: {}", req.network)))?;
		let wallet_id = Self::resolve_wallet(&req.from_user_id)?;
		let token = parse_tron_address(&req.token_contract).ok_or_else(|| Status::invalid_argument("token_contract must be a base58 Tron address"))?;
		let to = parse_tron_address(&req.to_address).ok_or_else(|| Status::invalid_argument("to_address must be a base58 Tron address"))?;
		let amount: u128 = req.amount.parse().map_err(|_| Status::invalid_argument("amount must be a u128 decimal"))?;
		let tx_ref = parse_tron_ref(&req.ref_block_bytes, &req.ref_block_hash, req.expiration, req.timestamp)?;
		self.guard_treasury_transfer(wallet_id, network, &req.to_address, amount)?;

		let secret = self.unseal(wallet_id, network).await?;
		let signed = tron_tx::sign_trc20_transfer(&secret, &token, &to, amount, req.fee_limit, &tx_ref).map_err(|_| Status::internal("signing failed"))?;
		Ok(Response::new(SignedTronTxResponse {
			signed_tx: signed.raw_tx,
			txid: signed.txid,
			expiration: req.expiration,
		}))
	}

	async fn sign_trx_transfer(&self, request: Request<SignTrxTransferRequest>) -> Result<Response<SignedTronTxResponse>, Status> {
		let req = request.into_inner();
		let network = Network::parse(&req.network).map_err(|_| Status::invalid_argument(format!("unknown network: {}", req.network)))?;
		let wallet_id = Self::resolve_wallet(&req.from_user_id)?;
		let to = parse_tron_address(&req.to_address).ok_or_else(|| Status::invalid_argument("to_address must be a base58 Tron address"))?;
		let amount: u128 = req.amount.parse().map_err(|_| Status::invalid_argument("amount must be a u128 decimal"))?;
		let tx_ref = parse_tron_ref(&req.ref_block_bytes, &req.ref_block_hash, req.expiration, req.timestamp)?;

		let secret = self.unseal(wallet_id, network).await?;
		let signed = tron_tx::sign_trx_transfer(&secret, &to, amount, &tx_ref).map_err(|_| Status::internal("signing failed"))?;
		Ok(Response::new(SignedTronTxResponse {
			signed_tx: signed.raw_tx,
			txid: signed.txid,
			expiration: req.expiration,
		}))
	}

	// === TON region (jetton USDT) =============================================
	async fn sign_jetton_transfer(&self, request: Request<SignJettonTransferRequest>) -> Result<Response<SignedTonTxResponse>, Status> {
		let req = request.into_inner();
		let network = require_ton(&req.network)?;
		let wallet_id = Self::resolve_wallet(&req.from_user_id)?;
		let amount: u128 = req.amount.parse().map_err(|_| Status::invalid_argument("amount must be a u128 decimal"))?;
		self.guard_treasury_transfer(wallet_id, network, &req.to_address, amount)?;

		let seed = self.unseal(wallet_id, network).await?;
		let signed = ton_tx::build_jetton_transfer(
			&seed,
			&ton_tx::JettonTransfer {
				our_jetton_wallet: &req.our_jetton_wallet,
				to_owner: &req.to_address,
				response_destination: &req.response_destination,
				amount,
				forward_ton_amount: req.forward_ton_amount,
				msg_value: req.msg_value,
				seqno: req.seqno,
				valid_until: req.valid_until,
			},
		)
		.map_err(ton_sign_status)?;
		Ok(Response::new(signed_ton_response(signed)))
	}

	async fn sign_ton_transfer(&self, request: Request<SignTonTransferRequest>) -> Result<Response<SignedTonTxResponse>, Status> {
		let req = request.into_inner();
		let network = require_ton(&req.network)?;
		let wallet_id = Self::resolve_wallet(&req.from_user_id)?;
		let amount: u128 = req.amount.parse().map_err(|_| Status::invalid_argument("amount must be a u128 decimal (nanotons)"))?;

		let seed = self.unseal(wallet_id, network).await?;
		let signed = ton_tx::build_native_transfer(
			&seed,
			&ton_tx::NativeTransfer {
				to_address: &req.to_address,
				amount,
				seqno: req.seqno,
				valid_until: req.valid_until,
			},
		)
		.map_err(ton_sign_status)?;
		Ok(Response::new(signed_ton_response(signed)))
	}
}

/// Parse a Base58Check `T…` Tron address into its 21-byte raw form, validating the checksum.
fn parse_tron_address(value: &str) -> Option<[u8; 21]> {
	crate::key_vault::tron_base58_to_raw(value)
}

/// Build the [`tron_tx::TxRef`] from the wire fields: the hub-fetched recent-block reference (hex)
/// plus the validity window. The ref-block hex must decode; the window is passed through.
fn parse_tron_ref(ref_block_bytes: &str, ref_block_hash: &str, expiration: i64, timestamp: i64) -> Result<tron_tx::TxRef, Status> {
	Ok(tron_tx::TxRef {
		ref_block_bytes: hex::decode(ref_block_bytes).map_err(|_| Status::invalid_argument("ref_block_bytes must be hex"))?,
		ref_block_hash: hex::decode(ref_block_hash).map_err(|_| Status::invalid_argument("ref_block_hash must be hex"))?,
		expiration,
		timestamp,
	})
}

/// Parse the wire network and require TON — the jetton/native TON signers unseal an
/// Ed25519 seed (`Chain::Ton`), so a non-TON network would mis-resolve the curve.
fn require_ton(raw: &str) -> Result<Network, Status> {
	match Network::parse(raw) {
		Ok(Network::Ton) => Ok(Network::Ton),
		Ok(other) => Err(Status::invalid_argument(format!("TON signer called with a non-TON network: {other}"))),
		Err(_) => Err(Status::invalid_argument(format!("unknown network: {raw}"))),
	}
}

/// A bad address is the caller's fault (invalid argument); a cell/serialize failure is
/// internal. Either way the seed never leaked — it stays in the [`ton_tx`] scope.
fn ton_sign_status(err: ton_tx::TonTxError) -> Status {
	match err {
		ton_tx::TonTxError::Address { .. } | ton_tx::TonTxError::Amount => Status::invalid_argument(err.to_string()),
		ton_tx::TonTxError::Cell(_) => Status::internal(err.to_string()),
	}
}

fn signed_ton_response(signed: ton_tx::SignedTonTx) -> SignedTonTxResponse {
	SignedTonTxResponse {
		signed_boc: signed.signed_boc,
		msg_hash: signed.msg_hash,
		seqno: signed.seqno,
		valid_until: signed.valid_until,
	}
}

/// Parse a `0x`-prefixed (or bare) hex string into a 20-byte EVM address. `None` if it is
/// not valid hex of exactly 20 bytes.
fn parse_evm_address(value: &str) -> Option<[u8; 20]> {
	let hex = value.strip_prefix("0x").unwrap_or(value);
	hex::decode(hex).ok()?.as_slice().try_into().ok()
}

#[cfg(test)]
mod tests {
	use super::parse_evm_address;

	#[test]
	fn parses_evm_addresses() {
		let addr = parse_evm_address("0x024da544a76714a3812096e9ef84d40b2c8863e8").unwrap();
		assert_eq!(addr[0], 0x02);
		assert_eq!(addr.len(), 20);
		// Bare (no 0x) is accepted too.
		assert_eq!(parse_evm_address("024da544a76714a3812096e9ef84d40b2c8863e8").unwrap(), addr);
		// Wrong length / non-hex are rejected.
		assert!(parse_evm_address("0x1234").is_none());
		assert!(parse_evm_address("0xnothex0000000000000000000000000000000000").is_none());
	}
}

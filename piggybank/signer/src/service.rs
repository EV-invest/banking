//! The gRPC driving adapter — the thin hub↔signer seam.
//!
//! It validates the wire request, unseals the relevant key transiently, and delegates the
//! key handling to the modules below it ([`provision`], [`evm_tx`], [`key_vault`]). A
//! plaintext key never leaves a handler. `Result<_, Status>` is tonic's mandated handler
//! signature and `Status` is a large type we don't control.
#![allow(clippy::result_large_err)]

use domain::money::Network;
use evbanking_contracts::signer::v1::{
	ProvisionAddressRequest, ProvisionAddressResponse, SignErc20TransferRequest, SignErc20TransferResponse, SignNativeTransferRequest, SignNativeTransferResponse,
	signer_service_server::SignerService,
};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{evm_tx, key_vault::Vault, provision, secrets::WalletSecrets};

/// The reserved wallet id for the treasury hot wallet. Real user ids are random v4 UUIDs
/// (never nil), so the treasury shares the `(user_id, network)` store without a schema
/// change. A withdrawal sends from here; a sweep sends INTO here from user addresses.
const TREASURY_WALLET: Uuid = Uuid::nil();

/// The signer service: the loaded [`Vault`] (holding the KEK) plus the
/// `wallet_secrets` store.
pub struct Signer {
	vault: Vault,
	secrets: WalletSecrets,
}

impl Signer {
	pub fn new(vault: Vault, secrets: WalletSecrets) -> Self {
		Self { vault, secrets }
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

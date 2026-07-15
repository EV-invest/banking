//! The gRPC driving adapter — the thin hub↔signer seam.
//!
//! It validates the wire request, unseals the relevant key transiently, and delegates the
//! key handling to the modules below it ([`provision`], [`evm_tx`], [`key_vault`]). A
//! plaintext key never leaves a handler. `Result<_, Status>` is tonic's mandated handler
//! signature and `Status` is a large type we don't control.
#![allow(clippy::result_large_err)]

use domain::money::Network;
use evbanking_contracts::signer::v1::{
	DeadKey, GetKeyHealthRequest, GetKeyHealthResponse, ProvisionAddressRequest, ProvisionAddressResponse, RotateAddressRequest, SignErc20TransferRequest, SignErc20TransferResponse,
	SignJettonTransferRequest, SignNativeTransferRequest, SignNativeTransferResponse, SignTonTransferRequest, SignTrc20TransferRequest, SignTrxTransferRequest, SignedTonTxResponse,
	SignedTronTxResponse, signer_service_server::SignerService,
};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{evm_tx, kek_guard::short_fp, key_vault::Vault, policy::SignerPolicy, provision, secrets::WalletSecrets, ton_tx, tron_tx};

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

	/// Apply the destination allowlist to a NATIVE (gas-coin) transfer signed FROM the
	/// treasury — the USDT cap cannot price a native amount, so only the allowlist applies.
	/// Gas top-ups are signed from the separate gas-station wallet and pass unchecked.
	fn guard_treasury_native_transfer(&self, wallet_id: Uuid, to_address: &str) -> Result<(), Status> {
		if wallet_id == TREASURY_WALLET {
			self.policy.check_treasury_native_transfer(to_address)?;
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

	/// Transiently unseal a sending wallet's 32-byte signing secret — a secp256k1 key
	/// (EVM/Tron) or a TON ed25519 seed — into a `Zeroizing` buffer (wiped on drop). The
	/// plaintext exists only for the duration of one signing call and never leaves this process.
	async fn unseal(&self, wallet_id: Uuid, network: Network) -> Result<zeroize::Zeroizing<[u8; 32]>, Status> {
		let sealed = self
			.secrets
			.find_sealed(wallet_id, network)
			.await?
			.ok_or_else(|| Status::failed_precondition("sending wallet is not provisioned"))?;
		let opened = self.vault.open(provision::chain_of(network), &sealed.id.to_string(), &sealed.sealed_key).map_err(|err| {
			// ERROR, not WARN: an unopenable key at sign time means funds are already
			// stranded on its address (the KEK-epoch bug class). GetKeyHealth lists it;
			// RotateAddress restores the user's ability to receive future deposits.
			tracing::error!(error = %err, %wallet_id, %network, "could not unseal the signing key — PROVABLY DEAD KEY, funds on its address cannot move");
			Status::internal("could not unseal the signing key")
		})?;
		Ok(zeroize::Zeroizing::new(<[u8; 32]>::try_from(opened.as_slice()).map_err(|_| {
			tracing::warn!(len = opened.len(), %wallet_id, %network, "stored key is not 32 bytes");
			Status::internal("stored key is not 32 bytes")
		})?))
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
		let network = require_evm(&req.network)?;
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
		.map_err(sign_status("erc20 transfer"))?;

		Ok(Response::new(SignErc20TransferResponse {
			raw_tx: format!("0x{}", hex::encode(&signed.raw)),
			tx_hash: format!("0x{}", hex::encode(signed.hash)),
		}))
	}

	async fn sign_native_transfer(&self, request: Request<SignNativeTransferRequest>) -> Result<Response<SignNativeTransferResponse>, Status> {
		let req = request.into_inner();
		let network = require_evm(&req.network)?;
		let wallet_id = Self::resolve_wallet(&req.from_user_id)?;
		let to = parse_evm_address(&req.to_address).ok_or_else(|| Status::invalid_argument("to_address must be a 0x 20-byte address"))?;
		let amount: u128 = req.amount.parse().map_err(|_| Status::invalid_argument("amount must be a u128 decimal"))?;
		let gas_price: u128 = req.gas_price.parse().map_err(|_| Status::invalid_argument("gas_price must be a u128 decimal"))?;
		self.guard_treasury_native_transfer(wallet_id, &req.to_address)?;

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
		.map_err(sign_status("native transfer"))?;

		Ok(Response::new(SignNativeTransferResponse {
			raw_tx: format!("0x{}", hex::encode(&signed.raw)),
			tx_hash: format!("0x{}", hex::encode(signed.hash)),
		}))
	}

	async fn sign_trc20_transfer(&self, request: Request<SignTrc20TransferRequest>) -> Result<Response<SignedTronTxResponse>, Status> {
		let req = request.into_inner();
		let network = require_tron(&req.network)?;
		let wallet_id = Self::resolve_wallet(&req.from_user_id)?;
		let token = parse_tron_address(&req.token_contract).ok_or_else(|| Status::invalid_argument("token_contract must be a base58 Tron address"))?;
		let to = parse_tron_address(&req.to_address).ok_or_else(|| Status::invalid_argument("to_address must be a base58 Tron address"))?;
		let amount: u128 = req.amount.parse().map_err(|_| Status::invalid_argument("amount must be a u128 decimal"))?;
		let tx_ref = parse_tron_ref(&req.ref_block_bytes, &req.ref_block_hash, req.expiration, req.timestamp)?;
		self.guard_treasury_transfer(wallet_id, network, &req.to_address, amount)?;

		let secret = self.unseal(wallet_id, network).await?;
		let signed = tron_tx::sign_trc20_transfer(&secret, &token, &to, amount, req.fee_limit, &tx_ref).map_err(sign_status("trc20 transfer"))?;
		Ok(Response::new(SignedTronTxResponse {
			signed_tx: signed.raw_tx,
			txid: signed.txid,
			expiration: req.expiration,
		}))
	}

	async fn sign_trx_transfer(&self, request: Request<SignTrxTransferRequest>) -> Result<Response<SignedTronTxResponse>, Status> {
		let req = request.into_inner();
		let network = require_tron(&req.network)?;
		let wallet_id = Self::resolve_wallet(&req.from_user_id)?;
		let to = parse_tron_address(&req.to_address).ok_or_else(|| Status::invalid_argument("to_address must be a base58 Tron address"))?;
		let amount: u128 = req.amount.parse().map_err(|_| Status::invalid_argument("amount must be a u128 decimal"))?;
		let tx_ref = parse_tron_ref(&req.ref_block_bytes, &req.ref_block_hash, req.expiration, req.timestamp)?;
		self.guard_treasury_native_transfer(wallet_id, &req.to_address)?;

		let secret = self.unseal(wallet_id, network).await?;
		let signed = tron_tx::sign_trx_transfer(&secret, &to, amount, &tx_ref).map_err(sign_status("trx transfer"))?;
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
		self.guard_treasury_native_transfer(wallet_id, &req.to_address)?;

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

	// === KEK-epoch diagnostics & recovery ======================================
	async fn get_key_health(&self, _request: Request<GetKeyHealthRequest>) -> Result<Response<GetKeyHealthResponse>, Status> {
		let fp = self.vault.fingerprint();
		let rows = self.secrets.active_epoch_rows().await?;
		let total_keys = rows.len() as u64;
		let mut dead_keys = Vec::new();
		for row in rows {
			let reason = match &row.kek_fp {
				Some(stamped) if stamped.as_slice() == fp => continue,
				Some(stamped) => format!("sealed under a different KEK epoch (fp {}, current {})", short_fp(stamped), short_fp(&fp)),
				// Pre-epoch row: probe it now; a survivor is stamped (healed) in place.
				None => match self.vault.open(provision::chain_of(row.network), &row.id.to_string(), &row.sealed_key) {
					Ok(_) => {
						self.secrets.stamp_kek_fp(row.id, &fp).await?;
						continue;
					}
					Err(_) => "cannot unseal under the current KEK".to_owned(),
				},
			};
			dead_keys.push(DeadKey {
				wallet_id: row.id.to_string(),
				user_id: row.user_id.to_string(),
				network: row.network.as_str().to_owned(),
				address: row.address,
				created_at: row.created_at,
				reason,
			});
		}
		Ok(Response::new(GetKeyHealthResponse {
			healthy_keys: total_keys - dead_keys.len() as u64,
			total_keys,
			dead_keys,
			kek_fingerprint: short_fp(&fp),
		}))
	}

	async fn rotate_address(&self, request: Request<RotateAddressRequest>) -> Result<Response<ProvisionAddressResponse>, Status> {
		let req = request.into_inner();
		let user_id = Uuid::parse_str(&req.user_id).map_err(|_| Status::invalid_argument("user_id must be a UUID"))?;
		let network = Network::parse(&req.network).map_err(|_| Status::invalid_argument(format!("unknown network: {}", req.network)))?;

		let sealed = self
			.secrets
			.find_sealed(user_id, network)
			.await?
			.ok_or_else(|| Status::failed_precondition("no active key for this (user, network) — nothing to rotate; provisioning will mint one"))?;
		// Rotation is a recovery tool for a provably dead key ONLY: a key that still
		// unseals keeps its address (rotating it would orphan a perfectly good key and
		// silently retire the address users may already be depositing to).
		if self.vault.open(provision::chain_of(network), &sealed.id.to_string(), &sealed.sealed_key).is_ok() {
			return Err(Status::failed_precondition("key is healthy under the current KEK — rotation refused"));
		}

		let old_address = self.secrets.find_address(user_id, network).await?.unwrap_or_default();
		if !self.secrets.supersede(user_id, network).await? {
			// Lost a race with a concurrent rotation — fall through: provision below
			// returns whatever active key now exists.
			tracing::warn!(%user_id, %network, "rotate: row already superseded by a concurrent rotation");
		}
		let provisioned = provision::provision(&self.vault, &self.secrets, user_id, network).await?;
		tracing::warn!(
			%user_id,
			%network,
			%old_address,
			new_address = %provisioned.address,
			"rotated a dead deposit key — the OLD address stays unspendable forever; only future deposits to the new address are safe"
		);
		Ok(Response::new(ProvisionAddressResponse {
			address: provisioned.address,
			address_kind: provisioned.kind.to_owned(),
		}))
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

/// Parse the wire network and require an EVM rail — the ERC-20/native signers unseal a
/// secp256k1 key and sign an EIP-155 legacy tx. The hub only ever passes its own EVM
/// `network`, but the signer is a separate trust domain and must not trust the string: a TON
/// network would feed an Ed25519 seed to `sign_legacy_tx` as a secp256k1 scalar (`from_slice`
/// accepts almost any 32 bytes → a signature from an unrelated address), and TRC20 — the same
/// curve — would sign from a Tron-scoped key the hub tracks at a different address.
fn require_evm(raw: &str) -> Result<Network, Status> {
	match Network::parse(raw) {
		Ok(network @ (Network::Bep20 | Network::Polygon)) => Ok(network),
		Ok(other) => Err(Status::invalid_argument(format!("EVM signer called with a non-EVM network: {other}"))),
		Err(_) => Err(Status::invalid_argument(format!("unknown network: {raw}"))),
	}
}

/// Parse the wire network and require a Tron rail — the TRC20/TRX signers unseal a secp256k1
/// key and sign a Tron tx, so a TON network would feed an Ed25519 seed to the secp256k1 signer
/// (the same curve-confusion hole `require_evm` guards on the EVM side).
fn require_tron(raw: &str) -> Result<Network, Status> {
	match Network::parse(raw) {
		Ok(Network::Trc20) => Ok(Network::Trc20),
		Ok(other) => Err(Status::invalid_argument(format!("Tron signer called with a non-Tron network: {other}"))),
		Err(_) => Err(Status::invalid_argument(format!("unknown network: {raw}"))),
	}
}

/// Log-then-withhold at a sign collapse point: the real cause goes to the server log, the
/// wire keeps the fixed non-leaking message.
fn sign_status<E: std::fmt::Display>(op: &'static str) -> impl Fn(E) -> Status {
	move |err| {
		tracing::warn!(error = %err, op, "signing failed");
		Status::internal("signing failed")
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
	use super::{parse_evm_address, require_evm, require_tron};

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

	#[test]
	fn require_evm_admits_only_evm_rails() {
		assert!(require_evm("bep20").is_ok());
		assert!(require_evm("polygon").is_ok());
		// TON is the curve-confusion case (Ed25519 seed → secp256k1); TRC20 shares the curve but
		// is a Tron-scoped key at a different address. Both must be refused, along with junk.
		assert!(require_evm("ton").is_err());
		assert!(require_evm("trc20").is_err());
		assert!(require_evm("bogus").is_err());
	}

	#[test]
	fn require_tron_admits_only_tron() {
		assert!(require_tron("trc20").is_ok());
		assert!(require_tron("ton").is_err());
		assert!(require_tron("bep20").is_err());
		assert!(require_tron("bogus").is_err());
	}
}

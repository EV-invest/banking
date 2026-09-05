//! Turnkey migration probe — a throwaway PoC, NOT part of the signer binary.
//!
//! The fund is weighing a move off this crate's own key storage (a KEK plus an
//! XChaCha20-Poly1305 envelope, see [`piggybank_signer::key_vault`]) and onto Turnkey, where
//! the private key never leaves a TEE enclave and signing happens over an API. Before a line
//! of migration code is written, three things have to be true, and this probe is what proves
//! them against a live organization:
//!
//! 1. Turnkey can hold all four networks we serve. Its account model is (curve, address
//!    format), so BSC/BEP20 and Polygon PoS share ONE `ADDRESS_FORMAT_ETHEREUM` secp256k1
//!    account, Tron gets `ADDRESS_FORMAT_TRON` on the same curve, and TON gets
//!    `ADDRESS_FORMAT_TON_V4R2` on ed25519.
//!
//! 2. Turnkey's address derivation is identical to ours. Every deposit address the fund has
//!    ever handed out came from `key_vault::{evm_address, tron_address, ton_address}`; if
//!    Turnkey derives a different address from the same public key, a migration would strand
//!    funds at addresses nobody watches. So we take Turnkey's public key, run it through OUR
//!    functions, and compare against the address Turnkey itself reports. A mismatch is a stop
//!    signal, and the probe exits non-zero on one.
//!
//! 3. A Turnkey signature over a PRE-COMPUTED 32-byte digest verifies locally against the
//!    account's public key. We sign a digest rather than a payload because all three of our
//!    transaction builders already hash: `evm_tx.rs:77` (keccak256 of the RLP), `tron_tx.rs:129`
//!    (sha256 of the protobuf, i.e. the txID) and `ton_tx.rs` (the cell representation hash).
//!    Turnkey must therefore not hash again — hence `HASH_FUNCTION_NO_OP` on secp256k1 and
//!    `HASH_FUNCTION_NOT_APPLICABLE` on ed25519, which has no prehash step to skip.
//!
//! It also settles one open question the migration code hangs on: what Turnkey puts in `v`.
//! `evm_tx.rs:82` computes `v = recovery_id + chain_id * 2 + 35` from a RAW 0/1 recovery id and
//! `tron_tx.rs` packs that raw byte as the 65th signature byte, so a Turnkey `v` of 27/28 would
//! need adjusting at both call sites. Rather than trust prose, the probe recovers the public key
//! under each interpretation and reports which one reproduces the signing key.
//!
//! Run it against a TEST organization — it creates a wallet and leaves it there:
//!
//! ```text
//! TURNKEY_API_PUBLIC_KEY=… TURNKEY_API_PRIVATE_KEY=… TURNKEY_ORGANIZATION_ID=… \
//!   cargo run -p piggybank-signer --example turnkey_poc
//! ```

use std::{env, str::FromStr};

use color_eyre::eyre::{Context, bail, eyre};
use ed25519_dalek::{Signature as Ed25519Signature, VerifyingKey as Ed25519VerifyingKey};
use k256::ecdsa::{RecoveryId, Signature as Secp256k1Signature, VerifyingKey as Secp256k1VerifyingKey, signature::hazmat::PrehashVerifier};
use piggybank_signer::key_vault;
use sha2::{Digest, Sha256};
use tonlib_core::TonAddress;
use turnkey_api_key_stamper::TurnkeyP256ApiKey;
use turnkey_client::{
	TurnkeyClient,
	generated::{
		CreateWalletIntent, GetWalletAccountsRequest, SignRawPayloadIntentV2, SignRawPayloadResult, WalletAccountParams,
		external::data::v1::WalletAccount,
		immutable::common::v1::{AddressFormat, Curve, HashFunction, PathFormat, PayloadEncoding},
	},
};

/// The two curves the fund actually uses. Turnkey's `Curve` also covers P-256 and an
/// unspecified variant; mapping through our own enum keeps every downstream `match`
/// exhaustive over the cases we support, so adding a chain breaks the build instead of
/// falling into a catch-all arm.
#[derive(Clone, Copy)]
enum Algo {
	Secp256k1,
	Ed25519,
}

impl Algo {
	fn curve(self) -> Curve {
		match self {
			Algo::Secp256k1 => Curve::Secp256k1,
			Algo::Ed25519 => Curve::Ed25519,
		}
	}

	/// Turnkey must NOT hash — our builders hand it a finished 32-byte digest.
	fn hash_function(self) -> HashFunction {
		match self {
			Algo::Secp256k1 => HashFunction::NoOp,
			// ed25519 signs the message itself, so there is no prehash step to switch off;
			// Turnkey spells that case with a different constant.
			Algo::Ed25519 => HashFunction::NotApplicable,
		}
	}
}

/// One Turnkey wallet account, plus the networks that single account backs for us.
struct AccountSpec {
	label: &'static str,
	algo: Algo,
	/// Turnkey's own default BIP-32 path for this address format (taken from the tkhq SDKs).
	/// A migration MUST reuse these paths or the same mnemonic re-derives different keys.
	path: &'static str,
	address_format: AddressFormat,
	/// Our derivation for this address format — the whole point of the cross-check.
	derive: fn(&[u8]) -> Option<String>,
	/// Normalizes an address string before comparison; see the free functions below for why
	/// each format needs a different rule.
	canonical: fn(&str) -> Option<String>,
	/// Report rows this account answers for: one secp256k1 EVM key serves BSC and Polygon alike.
	networks: &'static [&'static str],
}

const ACCOUNTS: &[AccountSpec] = &[
	AccountSpec {
		label: "EVM (secp256k1)",
		algo: Algo::Secp256k1,
		path: "m/44'/60'/0'/0/0",
		address_format: AddressFormat::Ethereum,
		derive: key_vault::evm_address,
		canonical: lowercased,
		networks: &["BSC/BEP20", "Polygon PoS"],
	},
	AccountSpec {
		label: "Tron (secp256k1)",
		algo: Algo::Secp256k1,
		path: "m/44'/195'/0'",
		address_format: AddressFormat::Tron,
		derive: key_vault::tron_address,
		canonical: verbatim,
		networks: &["Tron/TRC20"],
	},
	AccountSpec {
		label: "TON v4R2 (ed25519)",
		algo: Algo::Ed25519,
		path: "m/44'/607'/0'/0'/0'",
		address_format: AddressFormat::TonV4r2,
		derive: key_vault::ton_address,
		canonical: ton_canonical,
		networks: &["TON"],
	},
];

/// One line of the final go/no-go table.
struct Row {
	network: &'static str,
	address_matches: bool,
	signature_verified: bool,
}

/// What one account's probe established.
struct Outcome {
	address_matches: bool,
	signature_verified: bool,
	/// Present only for secp256k1: the empirically determined meaning of Turnkey's `v`.
	recovery_finding: Option<String>,
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
	color_eyre::install()?;
	dotenvy::dotenv().ok();

	let creds = Credentials::from_env()?;
	let api_key = TurnkeyP256ApiKey::from_strings(creds.api_private_key, Some(creds.api_public_key))
		.wrap_err("TURNKEY_API_PRIVATE_KEY / TURNKEY_API_PUBLIC_KEY are not a valid hex-encoded P-256 key pair")?;
	let client = TurnkeyClient::builder().api_key(api_key).build().wrap_err("failed to build the Turnkey client")?;

	// Content-derived so it is reproducible across runs and unmistakably not a real
	// transaction hash — this is exactly the shape our three builders already produce.
	let digest: [u8; 32] = Sha256::digest(b"evbanking-turnkey-poc:v1").into();

	let wallet_name = format!("evbanking-turnkey-poc-{}", client.current_timestamp());
	println!("creating HD wallet {wallet_name:?} in organization {}", creds.organization_id);
	let wallet_id = client
		.create_wallet(
			creds.organization_id.clone(),
			client.current_timestamp(),
			CreateWalletIntent {
				wallet_name,
				accounts: ACCOUNTS.iter().map(account_params).collect(),
				mnemonic_length: None,
			},
		)
		.await
		.wrap_err("create_wallet failed")?
		.result
		.wallet_id;
	println!("wallet {wallet_id} created (left in place — delete it when you are done)");
	println!("test digest: 0x{}\n", hex::encode(digest));

	// `create_wallet` returns addresses but not public keys, and the public key is what the
	// cross-check needs, so read the accounts back.
	let accounts = client
		.get_wallet_accounts(GetWalletAccountsRequest {
			organization_id: creds.organization_id.clone(),
			wallet_id: Some(wallet_id.clone()),
			include_wallet_details: None,
			pagination_options: None,
		})
		.await
		.wrap_err("list_wallet_accounts failed")?
		.accounts;

	let mut rows = Vec::new();
	let mut recovery_findings = Vec::new();
	for spec in ACCOUNTS {
		let account = accounts
			.iter()
			.find(|account| account.address_format == spec.address_format)
			.ok_or_else(|| eyre!("wallet {wallet_id} has no {:?} account — Turnkey did not create what we asked for", spec.address_format))?;

		let outcome = probe(&client, &creds.organization_id, spec, account, &digest).await?;
		if let Some(finding) = outcome.recovery_finding {
			recovery_findings.push(format!("{}: {finding}", spec.label));
		}
		rows.extend(spec.networks.iter().map(|network| Row {
			network,
			address_matches: outcome.address_matches,
			signature_verified: outcome.signature_verified,
		}));
	}

	println!("recovery id (`v`) — the value the migration code depends on");
	if recovery_findings.is_empty() {
		println!("  none: no secp256k1 signature was produced");
	}
	for finding in &recovery_findings {
		println!("  {finding}");
	}

	println!("\nsummary");
	println!("  {:<12} | address matched | signature verified", "network");
	for row in &rows {
		println!("  {:<12} | {:<15} | {}", row.network, yes_no(row.address_matches), yes_no(row.signature_verified));
	}

	let failed: Vec<&str> = rows.iter().filter(|row| !(row.address_matches && row.signature_verified)).map(|row| row.network).collect();
	if !failed.is_empty() {
		bail!(
			"STOP: Turnkey does not reproduce our derivation or signature for {} — do not start the migration",
			failed.join(", ")
		);
	}
	println!("\nall four networks agree with `key_vault` — the migration premise holds");
	Ok(())
}

/// Cross-check one account: our derivation against Turnkey's address, then a Turnkey
/// signature against Turnkey's public key.
///
/// A failing RPC aborts the whole probe (we cannot conclude anything without it), while a
/// failing check is recorded and printed — the point is to see all four verdicts in one run.
async fn probe(client: &TurnkeyClient<TurnkeyP256ApiKey>, organization_id: &str, spec: &AccountSpec, account: &WalletAccount, digest: &[u8; 32]) -> color_eyre::Result<Outcome> {
	println!("── {} — {}", spec.label, spec.path);

	let public_key_hex = account
		.public_key
		.as_deref()
		.ok_or_else(|| eyre!("Turnkey returned no public key for the {} account; the cross-check cannot run", spec.label))?;
	let public_key = decode_hex(public_key_hex).wrap_err_with(|| format!("Turnkey's public key for the {} account is not hex", spec.label))?;

	let ours = (spec.derive)(&public_key).ok_or_else(|| eyre!("key_vault rejected Turnkey's {} public key — it is not a key our derivation accepts", spec.label))?;
	let theirs = account.address.as_str();
	let address_matches = match ((spec.canonical)(&ours), (spec.canonical)(theirs)) {
		(Some(ours), Some(theirs)) => ours == theirs,
		// An unparseable address is itself a mismatch worth reporting, not a crash.
		_ => false,
	};
	println!("  turnkey address : {theirs}");
	println!("  key_vault       : {ours}");
	println!("  derivation      : {}", if address_matches { "MATCH" } else { "MISMATCH — STOP SIGNAL" });

	let signature = client
		.sign_raw_payload(
			organization_id.to_owned(),
			client.current_timestamp(),
			SignRawPayloadIntentV2 {
				sign_with: account.address.clone(),
				payload: format!("0x{}", hex::encode(digest)),
				encoding: PayloadEncoding::Hexadecimal,
				hash_function: spec.algo.hash_function(),
			},
		)
		.await
		.wrap_err_with(|| format!("sign_raw_payload failed for the {} account", spec.label))?
		.result;

	let verification = match spec.algo {
		Algo::Secp256k1 => verify_secp256k1(&public_key, digest, &signature),
		Algo::Ed25519 => verify_ed25519(&public_key, digest, &signature),
	};
	match &verification {
		Ok(()) => println!("  signature       : VERIFIED locally against Turnkey's public key"),
		Err(error) => println!("  signature       : FAILED to verify — {error:#}"),
	}

	let recovery_finding = match spec.algo {
		Algo::Secp256k1 => Some(classify_recovery_id(&public_key, digest, &signature).unwrap_or_else(|error| format!("could not be classified — {error:#}"))),
		// Ed25519 has no recovery id; Turnkey leaves `v` empty there.
		Algo::Ed25519 => None,
	};
	if let Some(finding) = &recovery_finding {
		println!("  v               : {finding}");
	}
	println!();

	Ok(Outcome {
		address_matches,
		signature_verified: verification.is_ok(),
		recovery_finding,
	})
}

/// Decide what Turnkey's `v` means by experiment: recover the public key under each candidate
/// interpretation and see which one reproduces the key that signed. This is the answer
/// `evm_tx.rs:82` and `tron_tx.rs` need, and guessing it wrong produces signatures that a node
/// accepts but attributes to the wrong sender.
fn classify_recovery_id(public_key: &[u8], digest: &[u8; 32], signature: &SignRawPayloadResult) -> color_eyre::Result<String> {
	let raw = parse_byte(&signature.v).ok_or_else(|| eyre!("v={:?} is neither a hex nor a decimal byte", signature.v))?;
	let parsed = secp256k1_signature(signature)?;
	let expected = Secp256k1VerifyingKey::from_sec1_bytes(public_key).wrap_err("Turnkey's public key is not a secp256k1 point")?;

	let recovers =
		|candidate: u8| RecoveryId::from_byte(candidate).is_some_and(|id| Secp256k1VerifyingKey::recover_from_prehash(digest, &parsed, id).is_ok_and(|recovered| recovered == expected));

	let verdict = if recovers(raw) {
		"RAW recovery id — `evm_tx.rs:82` (v = id + chain_id*2 + 35) and `tron_tx.rs` (raw 65th byte) take it as-is"
	} else if raw >= 27 && recovers(raw - 27) {
		"27/28 form — subtract 27 before `evm_tx.rs:82` and before packing Tron's 65th signature byte"
	} else {
		"NEITHER interpretation recovers the signing key — stop and investigate before migrating"
	};
	Ok(format!("{:?} (byte {raw}) → {verdict}", signature.v))
}

fn verify_secp256k1(public_key: &[u8], digest: &[u8; 32], signature: &SignRawPayloadResult) -> color_eyre::Result<()> {
	let parsed = secp256k1_signature(signature)?;
	let verifying_key = Secp256k1VerifyingKey::from_sec1_bytes(public_key).wrap_err("Turnkey's public key is not a secp256k1 point")?;
	// Both S forms verify against the same key, but chains only accept low-S and that is what
	// `k256` emits locally, so normalize before comparing like with like.
	let normalized = parsed.normalize_s().unwrap_or(parsed);
	verifying_key
		.verify_prehash(digest, &normalized)
		.wrap_err("the signature does not verify against Turnkey's own public key")
}

fn verify_ed25519(public_key: &[u8], digest: &[u8; 32], signature: &SignRawPayloadResult) -> color_eyre::Result<()> {
	let key: [u8; 32] = public_key
		.try_into()
		.map_err(|_| eyre!("Turnkey's ed25519 public key is {} bytes, expected 32", public_key.len()))?;
	let verifying_key = Ed25519VerifyingKey::from_bytes(&key).wrap_err("Turnkey's ed25519 public key is not a valid point")?;

	// Turnkey reuses the ECDSA `r`/`s` fields to carry the two halves of the 64-byte Ed25519
	// signature; `v` is empty there.
	let mut bytes = [0u8; 64];
	bytes[..32].copy_from_slice(&scalar32(&signature.r).wrap_err("Turnkey's r half is not 32 bytes")?);
	bytes[32..].copy_from_slice(&scalar32(&signature.s).wrap_err("Turnkey's s half is not 32 bytes")?);
	verifying_key
		.verify_strict(digest, &Ed25519Signature::from_bytes(&bytes))
		.wrap_err("the signature does not verify against Turnkey's own public key")
}

fn secp256k1_signature(signature: &SignRawPayloadResult) -> color_eyre::Result<Secp256k1Signature> {
	let mut bytes = [0u8; 64];
	bytes[..32].copy_from_slice(&scalar32(&signature.r).wrap_err("Turnkey's r is not a 32-byte scalar")?);
	bytes[32..].copy_from_slice(&scalar32(&signature.s).wrap_err("Turnkey's s is not a 32-byte scalar")?);
	Secp256k1Signature::from_slice(&bytes).wrap_err("Turnkey's (r, s) pair is not a valid secp256k1 signature")
}

fn account_params(spec: &AccountSpec) -> WalletAccountParams {
	WalletAccountParams {
		curve: spec.algo.curve(),
		path_format: PathFormat::Bip32,
		path: spec.path.to_owned(),
		address_format: spec.address_format,
		name: None,
	}
}

/// EIP-55 casing is a display checksum over an otherwise case-insensitive address, so a
/// casing difference is not a derivation difference.
fn lowercased(address: &str) -> Option<String> {
	Some(address.to_ascii_lowercase())
}

/// Tron's Base58Check IS case-sensitive — compare it byte for byte.
fn verbatim(address: &str) -> Option<String> {
	Some(address.to_owned())
}

/// A TON address has two equally valid renderings: `ton_address` returns the raw `0:<64hex>`
/// canonical form while Turnkey reports the user-friendly base64 one. Both parse to the same
/// (workchain, StateInit hash), so compare that.
fn ton_canonical(address: &str) -> Option<String> {
	TonAddress::from_str(address).ok().map(|parsed| parsed.to_hex())
}

/// Decodes a hex field, tolerating the `0x` prefix Turnkey uses on some of them.
fn decode_hex(value: &str) -> color_eyre::Result<Vec<u8>> {
	hex::decode(value.strip_prefix("0x").unwrap_or(value)).wrap_err("not valid hex")
}

/// Turnkey renders `r` and `s` as big integers, so a value with leading zero bytes can come
/// back short; both ECDSA and Ed25519 want them fixed at 32 bytes, hence the left-pad.
fn scalar32(value: &str) -> color_eyre::Result<[u8; 32]> {
	let bytes = decode_hex(value)?;
	if bytes.len() > 32 {
		bail!("expected at most 32 bytes, got {}", bytes.len());
	}
	let mut out = [0u8; 32];
	out[32 - bytes.len()..].copy_from_slice(&bytes);
	Ok(out)
}

/// `v` arrives hex-encoded ("00"/"01"), but accept a plain decimal too rather than fail the
/// classification on a formatting detail.
fn parse_byte(value: &str) -> Option<u8> {
	let trimmed = value.strip_prefix("0x").unwrap_or(value);
	match hex::decode(trimmed).ok().as_deref() {
		Some([byte]) => Some(*byte),
		_ => trimmed.parse().ok(),
	}
}

fn yes_no(value: bool) -> &'static str {
	if value { "yes" } else { "no" }
}

struct Credentials {
	api_public_key: String,
	api_private_key: String,
	organization_id: String,
}

impl Credentials {
	/// Reads the three variables the probe needs and names ALL the missing ones at once, so a
	/// half-configured shell takes one run to diagnose instead of three. Nothing read here is
	/// ever printed back except the organization id.
	fn from_env() -> color_eyre::Result<Self> {
		let mut missing = Vec::new();
		let credentials = Self {
			api_public_key: required("TURNKEY_API_PUBLIC_KEY", &mut missing),
			api_private_key: required("TURNKEY_API_PRIVATE_KEY", &mut missing),
			organization_id: required("TURNKEY_ORGANIZATION_ID", &mut missing),
		};
		if !missing.is_empty() {
			bail!(
				"not set in the environment: {}. The probe needs a Turnkey API key pair (hex) and the organization to create the test wallet in.",
				missing.join(", ")
			);
		}
		Ok(credentials)
	}
}

fn required(name: &'static str, missing: &mut Vec<&'static str>) -> String {
	match env::var(name) {
		Ok(value) if !value.trim().is_empty() => value,
		_ => {
			missing.push(name);
			String::new()
		}
	}
}

//! The Turnkey key backend — the second implementation of [`KeyBackend`], where the private
//! key lives inside a custodian's enclave and this process never holds one.
//!
//! Phase 2 of `docs/MIGRATION-turnkey.md`: compiled, tested, and **off** (`KEY_BACKEND`
//! defaults to `local`). The address formats and hash-function constants are lifted verbatim
//! from `examples/turnkey_poc.rs`, which established them against a live organization; the
//! derivation PATHS extend that probe's defaults with a per-user index (see
//! [`derivation_path`]) rather than reusing them verbatim, because one Turnkey wallet is now
//! shared across every user of a network (see [`network_wallet_name`]) instead of minting one
//! wallet per user.
//!
//! Four things in this module are worth more than the rest of it put together:
//!
//! 1. **One wallet per NETWORK, not per user** ([`network_wallet_name`]). Turnkey caps an
//!    organization at 100 HD Wallets but places no cap on HD Wallet Accounts — a wallet per
//!    `(user, network)` hits that ceiling at 25 users × 4 networks, and fights Turnkey's own
//!    model, where a wallet is a seed and an account is a free, unlimited derivation off it.
//! 2. **The error classification** ([`classify`]). `custody.rs` retries a withdrawal only on
//!    `Unavailable`/`DeadlineExceeded` and terminally fails it on anything else. A ten-second
//!    Turnkey outage misclassified as terminal is a failed withdrawal, so every transport
//!    failure, 5xx, rate limit and gateway hiccup maps to [`BackendError::Unavailable`], and
//!    only a refusal **on the merits** — a policy denial, a missing account, an activity that
//!    needs a human — is terminal.
//! 3. **`HASH_FUNCTION_NO_OP`** on secp256k1 and `NOT_APPLICABLE` on Ed25519. Our builders
//!    already hashed; a custodian that hashes again signs something the chain never asked
//!    about. See [`hash_function`].
//! 4. **The recovery id is ours**, from [`backend::recover_id`](crate::backend::recover_id) —
//!    never Turnkey's `v`, whose 0/1-vs-27/28 meaning is undocumented. Getting it wrong
//!    yields a signature a node accepts and credits to somebody else's address.
//!
//! Ed25519 gets no recovery-id check to fall out of, so signatures on that curve are
//! **verified locally** against the stored public key before they leave this module. That is
//! the same guarantee from the other side: a signature produced by the wrong account cannot
//! escape into a TON external message.

use std::{env, time::Duration};

use domain::money::Network;
use ed25519_dalek::{Signature as Ed25519Signature, VerifyingKey as Ed25519VerifyingKey};
use k256::ecdsa::Signature as Secp256k1Signature;
use tokio::{
	sync::Mutex,
	time::{Instant, sleep_until},
};
use turnkey_api_key_stamper::TurnkeyP256ApiKey;
use turnkey_client::{
	TurnkeyClient, TurnkeyClientError,
	generated::{
		CreateWalletAccountsIntent, CreateWalletIntent, GetWalletAccountRequest, SignRawPayloadIntentV2, SignRawPayloadResult, WalletAccountParams,
		external::data::v1::WalletAccount,
		immutable::common::v1::{AddressFormat, Curve as TurnkeyCurve, HashFunction, PathFormat, PayloadEncoding},
	},
};
use uuid::Uuid;

use crate::{
	backend::{BackendError, ChainSignature, Curve, KeyBackend, KeyHandle, recover_id},
	provision::{self, ProvisionedAddress},
	secrets::{NewTurnkeySecret, WalletSecrets},
};

/// Pay-as-you-go allows 1 request/second (Pro: 3). A batch sweep walks dozens of addresses
/// back to back, so the client serializes them with a margin rather than discovering the
/// limit as a 429 — which would land mid-sweep, on a rail with money on it.
const DEFAULT_MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(1_100);

/// A request that has not answered in this long is an outage, not a slow success: it must
/// surface as `Unavailable` and be retried, not hang a withdrawal worker indefinitely. The
/// SDK's own default is 20s; ours is explicit so the value is reviewable.
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Turnkey's account model is (curve, address format); the BIP-32/SLIP-10 derivation PATH is
/// deliberately NOT part of this spec. Every network has exactly ONE Turnkey wallet, shared by
/// every user (see [`network_wallet_name`]); what makes two users' accounts on the same
/// network different keys is [`derivation_path`], not a different wallet.
struct AccountSpec {
	curve: TurnkeyCurve,
	address_format: AddressFormat,
	/// The `key_alg` tag persisted on the row, matching what `provision::generate` writes for
	/// the local backend so the two backends' rows read alike.
	key_alg: &'static str,
}

/// The single place a network maps onto a Turnkey account shape. BEP20 and Polygon are both
/// EVM and share the `ADDRESS_FORMAT_ETHEREUM` spec; they still get distinct addresses because
/// each `(user, network)` gets its own [`derivation_path`], not because of anything here.
fn account_spec(network: Network) -> AccountSpec {
	match network {
		Network::Bep20 | Network::Polygon => AccountSpec {
			curve: TurnkeyCurve::Secp256k1,
			address_format: AddressFormat::Ethereum,
			key_alg: "secp256k1",
		},
		Network::Trc20 => AccountSpec {
			curve: TurnkeyCurve::Secp256k1,
			address_format: AddressFormat::Tron,
			key_alg: "secp256k1",
		},
		Network::Ton => AccountSpec {
			curve: TurnkeyCurve::Ed25519,
			address_format: AddressFormat::TonV4r2,
			key_alg: "ed25519",
		},
	}
}

/// The BIP-32 (secp256k1) / SLIP-0010 (Ed25519) derivation path for the `index`-th account on
/// `network` — the ONLY thing that makes two users' deposit addresses on the same network
/// different keys, now that every network has a single shared wallet (migration 0004). Each
/// arm starts from Turnkey's own default path for that network (taken from the tkhq SDKs and
/// confirmed live by the probe, see `examples/turnkey_poc.rs`) and varies exactly the
/// component a fresh account should vary:
///
/// - **EVM** (secp256k1): `m/44'/60'/0'/0/{index}` — standard BIP-44 external chain. The
///   trailing component is non-hardened in Turnkey's own default, and secp256k1 supports
///   non-hardened derivation, so `index` stays non-hardened too — the ordinary
///   `address_index` every EVM wallet already varies this way.
/// - **Tron** (secp256k1): `m/44'/195'/0'/0/{index}`. Turnkey's default (`m/44'/195'/0'`) stops
///   at the account level with no change/index component at all; we extend it with the same
///   standard BIP-44 external-chain shape as EVM rather than inventing a Tron-specific one —
///   secp256k1 again makes the non-hardened extension valid.
/// - **TON** (Ed25519): `m/44'/607'/0'/0'/{index}'`. Turnkey's default
///   (`m/44'/607'/0'/0'/0'`) is entirely hardened, and SLIP-0010 Ed25519 derivation supports
///   ONLY hardened components — there is no non-hardened form to fall back to. The varying
///   component must therefore carry the same trailing `'` as every other segment, or the path
///   is simply invalid for this curve.
fn derivation_path(network: Network, index: i64) -> String {
	match network {
		Network::Bep20 | Network::Polygon => format!("m/44'/60'/0'/0/{index}"),
		Network::Trc20 => format!("m/44'/195'/0'/0/{index}"),
		Network::Ton => format!("m/44'/607'/0'/0'/{index}'"),
	}
}

/// Turnkey must NOT hash — our three builders hand it a finished 32-byte digest
/// (`evm_tx` keccak256 of the RLP, `tron_tx` sha256 of the protobuf, `ton_tx` the cell hash).
/// Ed25519 signs the message itself and has no prehash step to switch off, which Turnkey
/// spells with a different constant; passing `NoOp` there is rejected.
fn hash_function(curve: Curve) -> HashFunction {
	match curve {
		Curve::Secp256k1 => HashFunction::NoOp,
		Curve::Ed25519 => HashFunction::NotApplicable,
	}
}

/// The ONE Turnkey wallet every user's account for `network` lives in.
///
/// Turnkey's org limits are 100 HD Wallets but UNLIMITED HD Wallet Accounts: a wallet per
/// `(user, network)` would hit the wallet cap at 25 users × 4 networks, and it fights
/// Turnkey's own model, where a wallet is a seed and an account is a free, unlimited
/// derivation off it. Four wallets total — one per network, named so an operator can find
/// each from `turnkey_network_wallets` (migration 0004) without a lookup — leaves an
/// enormous margin under the cap regardless of user count. BEP20 and Polygon still get
/// distinct addresses despite sharing the EVM `AccountSpec`: what makes them distinct is each
/// user's own [`derivation_path`], not a different wallet.
fn network_wallet_name(network: Network) -> String {
	format!("evbanking-{}", network.as_str())
}

/// A minimum-interval gate in front of every Turnkey call.
///
/// Serializes callers (the lock is held across the wait, so a burst queues instead of
/// stampeding) and spaces request *starts* by at least `interval`. Requests may still overlap
/// in flight — the limit is requests per second, not concurrency.
pub struct Throttle {
	interval: Duration,
	/// The earliest instant the next request may start. Guarded rather than atomic because the
	/// wait happens while holding it: that is what turns a burst into a queue.
	next_allowed: Mutex<Instant>,
}

impl Throttle {
	pub fn new(interval: Duration) -> Self {
		Self {
			interval,
			next_allowed: Mutex::new(Instant::now()),
		}
	}

	/// Wait until this caller's slot. Returns once the request may be issued.
	pub async fn acquire(&self) {
		let mut next_allowed = self.next_allowed.lock().await;
		let slot = (*next_allowed).max(Instant::now());
		sleep_until(slot).await;
		*next_allowed = slot + self.interval;
	}
}

/// The remote backend: a stamped HTTPS client, the organization every activity is scoped to,
/// the `wallet_secrets` store that maps `(wallet, network)` to a custodian handle, and the
/// rate gate.
///
/// **Not `Debug` on purpose.** The API private key is consumed into the stamper at
/// construction and never stored as a string, mirroring how `WALLET_KEK` stays out of
/// [`SignerConfig`](crate::config::SignerConfig).
pub struct TurnkeyBackend {
	client: TurnkeyClient<TurnkeyP256ApiKey>,
	organization_id: String,
	secrets: WalletSecrets,
	throttle: Throttle,
}

impl TurnkeyBackend {
	/// Build the backend from the environment. **Fail-fast and name every missing variable at
	/// once**, so a half-configured deployment takes one boot to diagnose instead of three.
	///
	/// - `TURNKEY_ORGANIZATION_ID`, `TURNKEY_API_PUBLIC_KEY`, `TURNKEY_API_PRIVATE_KEY` — required.
	/// - `TURNKEY_MIN_REQUEST_INTERVAL_MS` — the rate gate, default 1100ms (1 RPS with margin).
	/// - `TURNKEY_REQUEST_TIMEOUT_MS` — per-request timeout, default 30s.
	/// - `TURNKEY_BASE_URL` — staging/test escape hatch; production leaves it unset.
	pub fn from_env(secrets: WalletSecrets) -> color_eyre::Result<Self> {
		use color_eyre::eyre::{Context, bail};

		let mut missing = Vec::new();
		let organization_id = required("TURNKEY_ORGANIZATION_ID", &mut missing);
		let api_public_key = required("TURNKEY_API_PUBLIC_KEY", &mut missing);
		let api_private_key = required("TURNKEY_API_PRIVATE_KEY", &mut missing);
		if !missing.is_empty() {
			bail!(
				"KEY_BACKEND=turnkey but these are not set: {}. The signer needs a Turnkey API key pair (hex-encoded P-256) \
				 and the organization its wallets live in; set them or switch KEY_BACKEND back to `local`.",
				missing.join(", ")
			);
		}

		// The private key goes straight into the stamper and is never kept as a string.
		let api_key =
			TurnkeyP256ApiKey::from_strings(api_private_key, Some(api_public_key)).wrap_err("TURNKEY_API_PRIVATE_KEY / TURNKEY_API_PUBLIC_KEY are not a valid hex-encoded P-256 key pair")?;

		let mut builder = TurnkeyClient::builder()
			.api_key(api_key)
			.timeout(duration_ms("TURNKEY_REQUEST_TIMEOUT_MS", DEFAULT_REQUEST_TIMEOUT)?);
		if let Some(base_url) = env::var("TURNKEY_BASE_URL").ok().filter(|url| !url.trim().is_empty()) {
			tracing::warn!(%base_url, "TURNKEY_BASE_URL overrides the production Turnkey endpoint");
			builder = builder.base_url(base_url);
		}
		let client = builder.build().wrap_err("failed to build the Turnkey client")?;

		let interval = duration_ms("TURNKEY_MIN_REQUEST_INTERVAL_MS", DEFAULT_MIN_REQUEST_INTERVAL)?;
		tracing::info!(%organization_id, min_request_interval_ms = interval.as_millis(), "signer key backend: turnkey");
		Ok(Self {
			client,
			organization_id,
			secrets,
			throttle: Throttle::new(interval),
		})
	}

	/// Read back one account by its exact derivation path — the only way to identify a
	/// specific user's account once a network's shared wallet holds many. A single-account
	/// query (not the paginated list endpoint) so this stays correct however many users a
	/// network's wallet has accumulated. `create_wallet`/`create_wallet_accounts` return
	/// addresses but no public keys, and the public key is what every later signature is
	/// checked against, so the round trip is not optional.
	async fn wallet_account_at_path(&self, wallet_id: &str, path: &str) -> Result<WalletAccount, BackendError> {
		self.throttle.acquire().await;
		self.client
			.get_wallet_account(GetWalletAccountRequest {
				organization_id: self.organization_id.clone(),
				wallet_id: wallet_id.to_owned(),
				address: None,
				path: Some(path.to_owned()),
			})
			.await
			.map_err(|err| classify(&err, "get_wallet_account"))?
			.account
			.ok_or_else(|| BackendError::Protocol(format!("wallet {wallet_id} has no account at path {path} — Turnkey did not create what was asked for")))
	}

	/// Ensure `network`'s shared wallet has an account at `account.path`, creating the wallet
	/// itself first if this is the network's very first ever provision. Returns the wallet_id
	/// the account was actually minted in.
	///
	/// A concurrent bootstrap of the SAME network's first-ever wallet can lose the race to
	/// register it as canonical in `turnkey_network_wallets` — the loser's wallet (holding a
	/// perfectly valid account) is then simply never reused for a later user. That costs one
	/// extra wallet, at most once per network, ever: Turnkey resolves `sign_with` by account
	/// address, not by wallet, so signing is unaffected either way. The same bounded,
	/// harmless waste the old per-user design already tolerated (see [`network_wallet_name`]).
	async fn ensure_account(&self, network: Network, account: WalletAccountParams) -> Result<String, BackendError> {
		if let Some(wallet_id) = self.secrets.network_wallet_id(network).await? {
			self.throttle.acquire().await;
			self.client
				.create_wallet_accounts(
					self.organization_id.clone(),
					self.client.current_timestamp(),
					CreateWalletAccountsIntent {
						wallet_id: wallet_id.clone(),
						accounts: vec![account],
						persist: Some(true),
					},
				)
				.await
				.map_err(|err| classify(&err, "create_wallet_accounts"))?;
			return Ok(wallet_id);
		}

		// Nobody has provisioned on this network yet: mint the shared wallet, seeded with
		// THIS user's account so the very first call needs only one activity, exactly like
		// creating any other wallet account.
		self.throttle.acquire().await;
		let wallet_id = self
			.client
			.create_wallet(
				self.organization_id.clone(),
				self.client.current_timestamp(),
				CreateWalletIntent {
					wallet_name: network_wallet_name(network),
					accounts: vec![account],
					mnemonic_length: None,
				},
			)
			.await
			.map_err(|err| classify(&err, "create_wallet"))?
			.result
			.wallet_id;
		self.secrets.insert_network_wallet(network, &wallet_id).await?;
		Ok(wallet_id)
	}
}

#[tonic::async_trait]
impl KeyBackend for TurnkeyBackend {
	/// Mint (or return the existing) custody-held deposit address for `(user, network)`.
	///
	/// Idempotency comes from `wallet_secrets`, exactly as it does for the local vault: an
	/// existing active row short-circuits before any network call, any sequence pull, and any
	/// account creation. A race on a network's very first-ever provision can leave one
	/// orphaned Turnkey wallet whose account never left this function — wasteful, never
	/// dangerous (see [`ensure_account`](Self::ensure_account)), and the losing
	/// `insert_network_wallet` is a no-op.
	async fn provision(&self, user_id: Uuid, network: Network) -> Result<ProvisionedAddress, BackendError> {
		if let Some((stored, public_key)) = self.secrets.find_watch(user_id, network).await? {
			let (address, kind) = provision::render_address(network, &public_key)?;
			if address != stored {
				self.secrets.update_address(user_id, network, &address).await?;
			}
			return Ok(ProvisionedAddress { address, kind });
		}

		let spec = account_spec(network);
		// The index (and the path it feeds) is pulled ONLY on this not-yet-provisioned path —
		// the idempotency check above already returned for a repeat call, so a re-provision of
		// the same (user, network) never touches the sequence, never mints a second account,
		// and never risks a different address for the same row.
		let index = self.secrets.next_derivation_index().await?;
		let path = derivation_path(network, index);
		let account_params = WalletAccountParams {
			curve: spec.curve,
			path_format: PathFormat::Bip32,
			path: path.clone(),
			address_format: spec.address_format,
			name: None,
		};

		let wallet_id = self.ensure_account(network, account_params).await?;
		let account = self.wallet_account_at_path(&wallet_id, &path).await?;
		let public_key = decode_hex(
			account
				.public_key
				.as_deref()
				.ok_or_else(|| BackendError::Protocol("Turnkey returned an account with no public key".to_owned()))?,
		)
		.ok_or_else(|| BackendError::Protocol("Turnkey's account public key is not hex".to_owned()))?;
		let (address, _) = provision::render_address(network, &public_key)?;

		// The probe's cross-check, kept on forever. It is the same guarantee the local vault's
		// unseal-probe gives: nothing leaves this function as a fundable address until we have
		// proven we can reproduce it ourselves. A silent derivation drift would otherwise hand
		// users an address the indexer never watches.
		if !addresses_agree(network, &address, &account.address) {
			tracing::error!(%user_id, %network, ours = %address, theirs = %account.address, "Turnkey's address does not match our derivation — refusing to provision");
			return Err(BackendError::Protocol("Turnkey's address does not match our own derivation".to_owned()));
		}

		self.secrets
			.insert_turnkey(&NewTurnkeySecret {
				id: Uuid::new_v4(),
				user_id,
				network,
				public_key: &public_key,
				address: &address,
				sign_with: &account.address,
				key_alg: spec.key_alg,
				derivation_index: index,
			})
			.await?;

		// Re-read the canonical row: ours, or a concurrent racer's whose insert won. Derive from
		// its public key so the returned address always matches the persisted row.
		let (stored, public_key) = self
			.secrets
			.find_watch(user_id, network)
			.await?
			.ok_or_else(|| BackendError::Protocol("wallet_secrets row missing immediately after insert".to_owned()))?;
		let (address, kind) = provision::render_address(network, &public_key)?;
		if address != stored {
			self.secrets.update_address(user_id, network, &address).await?;
		}
		Ok(ProvisionedAddress { address, kind })
	}

	async fn public_key(&self, handle: KeyHandle) -> Result<Vec<u8>, BackendError> {
		let (_, public_key) = self.secrets.find_watch(handle.wallet_id, handle.network).await?.ok_or(BackendError::NotProvisioned)?;
		Ok(public_key)
	}

	async fn sign_digest(&self, handle: KeyHandle, curve: Curve, digest: &[u8; 32]) -> Result<ChainSignature, BackendError> {
		if Curve::of(handle.network) != curve {
			return Err(BackendError::CurveMismatch { network: handle.network });
		}
		let key = match self.secrets.find_turnkey(handle.wallet_id, handle.network).await? {
			Some(Ok(key)) => key,
			// Phase 3 dispatches per row and sends this one to the local vault; until then,
			// refuse rather than ask Turnkey for a key it has never held.
			Some(Err(stored)) => return Err(BackendError::WrongBackend { stored }),
			None => return Err(BackendError::NotProvisioned),
		};

		self.throttle.acquire().await;
		let signature = self
			.client
			.sign_raw_payload(
				self.organization_id.clone(),
				self.client.current_timestamp(),
				SignRawPayloadIntentV2 {
					sign_with: key.sign_with,
					payload: format!("0x{}", hex::encode(digest)),
					encoding: PayloadEncoding::Hexadecimal,
					hash_function: hash_function(curve),
				},
			)
			.await
			.map_err(|err| classify(&err, "sign_raw_payload"))?
			.result;

		to_chain_signature(curve, &signature, &key.public_key, digest)
	}
}

/// Turn Turnkey's `(r, s, v)` into the shape the assemblers need, checking on the way that the
/// signature really came from the account we believe signed it.
///
/// `v` is discarded. On secp256k1 the recovery id is re-derived locally, which doubles as that
/// check — [`recover_id`] returns `None` unless `(r, s)` recovers the stored public key. On
/// Ed25519 there is no recovery id, so the check is an explicit strict verify.
fn to_chain_signature(curve: Curve, signature: &SignRawPayloadResult, public_key: &[u8], digest: &[u8; 32]) -> Result<ChainSignature, BackendError> {
	let r = scalar32(&signature.r).ok_or_else(|| BackendError::Protocol("Turnkey's r is not a 32-byte scalar".to_owned()))?;
	let s = scalar32(&signature.s).ok_or_else(|| BackendError::Protocol("Turnkey's s is not a 32-byte scalar".to_owned()))?;

	match curve {
		Curve::Secp256k1 => {
			let mut compact = [0u8; 64];
			compact[..32].copy_from_slice(&r);
			compact[32..].copy_from_slice(&s);
			let parsed = Secp256k1Signature::from_slice(&compact).map_err(|_| BackendError::Protocol("Turnkey's (r, s) pair is not a valid secp256k1 signature".to_owned()))?;
			// Chains accept only low-S, and `k256` normalizes locally, so normalize here too —
			// before deriving the recovery id, which is defined against the emitted (r, s).
			let bytes = parsed.normalize_s().unwrap_or(parsed).to_bytes();
			let mut r = [0u8; 32];
			let mut s = [0u8; 32];
			r.copy_from_slice(&bytes[..32]);
			s.copy_from_slice(&bytes[32..]);
			let recovery_id = recover_id(&r, &s, digest, public_key).ok_or_else(|| {
				// Not `Unavailable`: a signature that does not recover the stored key is a wrong
				// key or a wrong digest, and retrying reproduces it. It must never be assembled —
				// the chain would accept it and credit a different sender.
				BackendError::Rejected("Turnkey's signature does not recover the account's stored public key".to_owned())
			})?;
			Ok(ChainSignature::Ecdsa { r, s, recovery_id })
		}
		Curve::Ed25519 => {
			let key: [u8; 32] = public_key
				.try_into()
				.map_err(|_| BackendError::Protocol(format!("stored Ed25519 public key is {} bytes, expected 32", public_key.len())))?;
			let verifying = Ed25519VerifyingKey::from_bytes(&key).map_err(|_| BackendError::Protocol("stored Ed25519 public key is not a valid point".to_owned()))?;
			let mut bytes = [0u8; 64];
			bytes[..32].copy_from_slice(&r);
			bytes[32..].copy_from_slice(&s);
			// `verify_strict` over `verify`: it rejects small-order and torsion-component keys,
			// which is the same fail-closed stance the secp256k1 branch gets from `recover_id`.
			verifying
				.verify_strict(digest, &Ed25519Signature::from_bytes(&bytes))
				.map_err(|_| BackendError::Rejected("Turnkey's Ed25519 signature does not verify against the account's stored public key".to_owned()))?;
			Ok(ChainSignature::Ed25519(bytes))
		}
	}
}

/// Does Turnkey's rendering of an address mean the same thing as ours?
///
/// Each format needs its own rule, and the probe settled all three: EIP-55 casing is a display
/// checksum over a case-insensitive address; Tron's Base58Check IS case-sensitive; a TON
/// address has two equally valid renderings (our raw `0:<64hex>` and Turnkey's user-friendly
/// base64) that parse to the same (workchain, StateInit hash).
fn addresses_agree(network: Network, ours: &str, theirs: &str) -> bool {
	match network {
		Network::Bep20 | Network::Polygon => ours.eq_ignore_ascii_case(theirs),
		Network::Trc20 => ours == theirs,
		Network::Ton => {
			use std::str::FromStr as _;
			match (tonlib_core::TonAddress::from_str(ours), tonlib_core::TonAddress::from_str(theirs)) {
				(Ok(ours), Ok(theirs)) => ours == theirs,
				// An unparseable address is itself a disagreement, not a crash.
				_ => false,
			}
		}
	}
}

/// Which side of the retry/terminal split a Turnkey failure falls on — the single most
/// consequential decision in this module.
///
/// The asymmetry is deliberate. Calling a transient failure terminal turns a brief outage into
/// a failed withdrawal that needs a human; calling a terminal failure transient costs some
/// wasted retries against a refusal that will not change. So anything that could plausibly be
/// the network, a gateway, or Turnkey having a bad minute is [`BackendError::Unavailable`], and
/// only an answer that says "no" about THIS request is terminal.
///
/// `op` names the call site for the log; the returned message is a category we compose, never
/// a response body (which can echo the request back).
fn classify(err: &TurnkeyClientError, op: &'static str) -> BackendError {
	tracing::warn!(error = %err, op, "turnkey call failed");
	match err {
		// Timeouts, dropped connections, TLS failures, truncated bodies. Always transient.
		TurnkeyClientError::Http(_) => BackendError::Unavailable(format!("{op}: transport failure")),

		TurnkeyClientError::UnexpectedHttpStatus(status, _) => match *status {
			// 429 is the rate limit the throttle exists to avoid; 408/425 are the server asking
			// to try again; 5xx is Turnkey, not us.
			408 | 425 | 429 => BackendError::Unavailable(format!("{op}: HTTP {status} (rate limited or timed out)")),
			500..=599 => BackendError::Unavailable(format!("{op}: HTTP {status}")),
			// A rejected stamp is a credential or clock problem — an operational fault, not a
			// verdict on this withdrawal. Retrying lets a re-issued key or a corrected clock
			// heal it; failing the transfer would not.
			401 => BackendError::Unavailable(format!("{op}: HTTP {status} (stamp rejected — check the API key and clock skew)")),
			// 403 policy denial, 404 unknown account, 400 malformed intent: answers about this
			// request, reproduced exactly by a retry.
			_ => BackendError::Rejected(format!("{op}: HTTP {status}")),
		},

		// The activity was accepted and stayed PENDING past the SDK's retries. Nothing was
		// finalized against us and the same request can be re-issued.
		TurnkeyClientError::ExceededRetries(attempts) => BackendError::Unavailable(format!("{op}: activity still pending after {attempts} retries")),

		// A redirect we refused to follow, or a response that is not the JSON the API promises:
		// the signature of a proxy, captive portal or gateway between us and Turnkey.
		TurnkeyClientError::RefusedRedirect(..)
		| TurnkeyClientError::MissingContentTypeHeader
		| TurnkeyClientError::UnexpectedMimeType(_)
		| TurnkeyClientError::HeaderToStrError(_)
		| TurnkeyClientError::HeaderFromStrError(_) => BackendError::Unavailable(format!("{op}: response did not come from the Turnkey API")),

		// The activity ran and failed. Turnkey states the reason as a google.rpc code, so honour
		// it: its own retryable codes stay retryable, everything else is a refusal on the merits
		// (the policy engine being the common case).
		TurnkeyClientError::ActivityFailed(failure) => match failure.as_ref().map(|status| status.code) {
			// DEADLINE_EXCEEDED, RESOURCE_EXHAUSTED, ABORTED, UNAVAILABLE.
			Some(code @ (4 | 8 | 10 | 14)) => BackendError::Unavailable(format!("{op}: activity failed with retryable code {code}")),
			Some(code) => BackendError::Rejected(format!("{op}: activity failed with code {code}")),
			None => BackendError::Rejected(format!("{op}: activity failed without a status")),
		},

		// Consensus or an extra authenticator is required: a human must act, and re-issuing the
		// same activity will not summon them.
		TurnkeyClientError::ActivityRequiresApproval(id) => BackendError::Rejected(format!("{op}: activity {id} requires approval")),
		TurnkeyClientError::UnexpectedActivityStatus(status) => BackendError::Rejected(format!("{op}: activity status {status}")),

		// Schema drift between this SDK and the API, or a result of the wrong shape. Will not
		// heal on its own; an operator has to look.
		TurnkeyClientError::Decode(..)
		| TurnkeyClientError::SerdeJsonFailure(_)
		| TurnkeyClientError::MissingActivity
		| TurnkeyClientError::MissingResult
		| TurnkeyClientError::MissingInnerResult
		| TurnkeyClientError::UnexpectedInnerActivityResult(_) => BackendError::Protocol(format!("{op}: unreadable Turnkey response")),

		// Local misconfiguration — the client should not have been built at all.
		TurnkeyClientError::BuilderMissingApiKey | TurnkeyClientError::ReqwestBuilder(_) | TurnkeyClientError::StamperError(_) => {
			BackendError::Protocol(format!("{op}: local Turnkey client failure"))
		}
	}
}

/// Turnkey renders `r`/`s` as big integers, so a value with leading zero bytes comes back
/// short; both curves want them fixed at 32 bytes, hence the left-pad.
fn scalar32(value: &str) -> Option<[u8; 32]> {
	let bytes = decode_hex(value)?;
	if bytes.len() > 32 {
		return None;
	}
	let mut out = [0u8; 32];
	out[32 - bytes.len()..].copy_from_slice(&bytes);
	Some(out)
}

/// Decodes a hex field, tolerating the `0x` prefix Turnkey uses on some of them.
fn decode_hex(value: &str) -> Option<Vec<u8>> {
	hex::decode(value.strip_prefix("0x").unwrap_or(value)).ok()
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

fn duration_ms(name: &str, default: Duration) -> color_eyre::Result<Duration> {
	use color_eyre::eyre::Context as _;

	match env::var(name).ok().filter(|raw| !raw.trim().is_empty()) {
		Some(raw) => Ok(Duration::from_millis(
			raw.trim().parse().with_context(|| format!("{name} must be a whole number of milliseconds"))?,
		)),
		None => Ok(default),
	}
}

#[cfg(test)]
mod tests {
	use tonic::{Code, Status};
	use turnkey_client::generated::{GetWalletsRequest, google::rpc::Status as RpcStatus};

	use super::*;

	/// The classification is what decides whether an outage costs a retry or a withdrawal, so
	/// assert it where it actually lands: the gRPC code `custody.rs` branches on.
	fn code_for(err: &TurnkeyClientError) -> Code {
		Status::from(classify(err, "test")).code()
	}

	#[test]
	fn transient_turnkey_failures_stay_retryable() {
		for err in [
			TurnkeyClientError::UnexpectedHttpStatus(500, "boom".into()),
			TurnkeyClientError::UnexpectedHttpStatus(502, "bad gateway".into()),
			TurnkeyClientError::UnexpectedHttpStatus(503, "unavailable".into()),
			TurnkeyClientError::UnexpectedHttpStatus(504, "gateway timeout".into()),
			TurnkeyClientError::UnexpectedHttpStatus(429, "slow down".into()),
			TurnkeyClientError::UnexpectedHttpStatus(408, "request timeout".into()),
			TurnkeyClientError::UnexpectedHttpStatus(401, "bad stamp".into()),
			TurnkeyClientError::ExceededRetries(5),
			TurnkeyClientError::RefusedRedirect(302, "http://elsewhere".into()),
			TurnkeyClientError::MissingContentTypeHeader,
			TurnkeyClientError::UnexpectedMimeType("text/html".into()),
			TurnkeyClientError::HeaderToStrError("bad header".into()),
			TurnkeyClientError::HeaderFromStrError("bad header".into()),
			TurnkeyClientError::ActivityFailed(Some(RpcStatus {
				code: 14,
				message: "unavailable".into(),
				details: Vec::new(),
			})),
		] {
			assert_eq!(code_for(&err), Code::Unavailable, "must be retryable, else a brief outage fails a withdrawal: {err}");
		}
	}

	#[test]
	fn refusals_on_the_merits_are_terminal() {
		for err in [
			// The policy engine, an unknown account, a malformed intent.
			TurnkeyClientError::UnexpectedHttpStatus(403, "policy denied".into()),
			TurnkeyClientError::UnexpectedHttpStatus(404, "no such account".into()),
			TurnkeyClientError::UnexpectedHttpStatus(400, "bad request".into()),
			TurnkeyClientError::ActivityFailed(Some(RpcStatus {
				code: 7,
				message: "permission denied".into(),
				details: Vec::new(),
			})),
			TurnkeyClientError::ActivityFailed(None),
			TurnkeyClientError::ActivityRequiresApproval("act-1".into()),
			TurnkeyClientError::UnexpectedActivityStatus("ACTIVITY_STATUS_REJECTED".into()),
		] {
			assert_ne!(code_for(&err), Code::Unavailable, "a refusal on the merits must not be retried forever: {err}");
			assert_ne!(code_for(&err), Code::DeadlineExceeded, "DeadlineExceeded is also retried by custody.rs: {err}");
		}
	}

	#[test]
	fn unreadable_responses_are_internal_not_retried() {
		for err in [
			TurnkeyClientError::MissingActivity,
			TurnkeyClientError::MissingResult,
			TurnkeyClientError::MissingInnerResult,
			TurnkeyClientError::UnexpectedInnerActivityResult("wrong shape".into()),
			TurnkeyClientError::BuilderMissingApiKey,
		] {
			assert_eq!(code_for(&err), Code::Internal, "{err}");
		}
	}

	/// The `Http` arm is the one that carries real timeouts and dropped connections, and
	/// `reqwest::Error` has no public constructor — so produce a genuine one by pointing the
	/// SDK at a closed local port. No network, no credentials, no Turnkey account.
	#[tokio::test]
	async fn a_real_transport_failure_is_unavailable() {
		let client = TurnkeyClient::builder()
			.api_key(TurnkeyP256ApiKey::generate())
			.base_url("http://127.0.0.1:1")
			.timeout(Duration::from_millis(500))
			.build()
			.expect("client builds");
		let err = client
			.get_wallets(GetWalletsRequest {
				organization_id: "org-does-not-matter".to_owned(),
			})
			.await
			.expect_err("port 1 on loopback refuses connections");
		assert!(matches!(err, TurnkeyClientError::Http(_)), "expected a transport error, got {err}");
		assert_eq!(code_for(&err), Code::Unavailable);
	}

	/// A batch sweep walks dozens of addresses; the gate must space them, not let them burst.
	/// Paused time makes the assertion exact instead of timing-dependent.
	#[tokio::test(start_paused = true)]
	async fn throttle_spaces_sequential_calls() {
		let throttle = Throttle::new(Duration::from_millis(1_100));
		let start = Instant::now();
		for _ in 0..4 {
			throttle.acquire().await;
		}
		// Four slots, three gaps — the first request goes out immediately.
		assert!(
			start.elapsed() >= Duration::from_millis(3_300),
			"four calls must be spaced by three full intervals, got {:?}",
			start.elapsed()
		);
		assert!(start.elapsed() < Duration::from_millis(3_400), "and must not over-throttle, got {:?}", start.elapsed());
	}

	#[tokio::test(start_paused = true)]
	async fn throttle_serializes_a_concurrent_burst() {
		let throttle = std::sync::Arc::new(Throttle::new(Duration::from_millis(1_000)));
		let start = Instant::now();
		let mut set = tokio::task::JoinSet::new();
		for _ in 0..5 {
			let throttle = std::sync::Arc::clone(&throttle);
			set.spawn(async move { throttle.acquire().await });
		}
		while let Some(joined) = set.join_next().await {
			joined.expect("throttle task must not panic");
		}
		assert!(
			start.elapsed() >= Duration::from_millis(4_000),
			"a burst of 5 must be serialized into 4 gaps, not fired at once — got {:?}",
			start.elapsed()
		);
	}

	#[test]
	fn every_network_maps_to_the_probe_verified_account_spec() {
		// These formats came from a live Turnkey organization via the PoC. A change here
		// re-derives a different address shape entirely — it must be deliberate.
		let evm = account_spec(Network::Bep20);
		assert_eq!(evm.address_format, AddressFormat::Ethereum);
		assert_eq!(account_spec(Network::Polygon).address_format, evm.address_format, "Polygon and BEP20 share the EVM account spec");
		let tron = account_spec(Network::Trc20);
		assert_eq!(tron.address_format, AddressFormat::Tron);
		let ton = account_spec(Network::Ton);
		assert_eq!((ton.address_format, ton.curve), (AddressFormat::TonV4r2, TurnkeyCurve::Ed25519));
	}

	/// Each network's path form, pinned exactly — the whole point of the fix that replaced a
	/// wallet-per-user with a shared wallet + a per-account derivation index. A change here
	/// re-derives a different key at the same index; each arm must stay a strict superset of
	/// the probe-verified default (`docs/MIGRATION-turnkey.md`, decision "1 wallet per
	/// network").
	#[test]
	fn derivation_paths_extend_each_networks_probe_verified_default() {
		// EVM: the probe's default IS "index 0" of the standard non-hardened BIP-44 chain —
		// secp256k1 supports non-hardened derivation, so only the trailing component varies.
		assert_eq!(derivation_path(Network::Bep20, 0), "m/44'/60'/0'/0/0");
		assert_eq!(derivation_path(Network::Bep20, 7), "m/44'/60'/0'/0/7");
		assert_eq!(derivation_path(Network::Polygon, 7), "m/44'/60'/0'/0/7", "Polygon shares the EVM path shape");

		// Tron: the probe's default (`m/44'/195'/0'`) has no chain/index component to vary, so
		// index 0 is NOT byte-identical to the probe's own path — it is the same account
		// extended with the standard BIP-44 external chain, which secp256k1 also allows
		// non-hardened.
		assert_eq!(derivation_path(Network::Trc20, 0), "m/44'/195'/0'/0/0");
		assert_eq!(derivation_path(Network::Trc20, 7), "m/44'/195'/0'/0/7");

		// TON: Ed25519/SLIP-0010 has no non-hardened form at all, so the varying component MUST
		// carry the trailing `'` like every other segment in the probe's all-hardened default.
		assert_eq!(derivation_path(Network::Ton, 0), "m/44'/607'/0'/0'/0'");
		assert_eq!(derivation_path(Network::Ton, 7), "m/44'/607'/0'/0'/7'");
	}

	/// Re-hashing a digest we already hashed signs something the chain never asked about.
	#[test]
	fn hash_function_never_lets_turnkey_hash_again() {
		assert_eq!(hash_function(Curve::Secp256k1), HashFunction::NoOp);
		assert_eq!(hash_function(Curve::Ed25519), HashFunction::NotApplicable);
	}

	#[test]
	fn address_agreement_follows_each_chains_own_rule() {
		// EIP-55 casing is a display checksum, not a different address.
		assert!(addresses_agree(
			Network::Bep20,
			"0xAbCd0000000000000000000000000000000000Ef",
			"0xabcd0000000000000000000000000000000000ef"
		));
		assert!(!addresses_agree(
			Network::Polygon,
			"0xabcd0000000000000000000000000000000000ef",
			"0xabcd0000000000000000000000000000000000e0"
		));
		// Tron's Base58Check alphabet is case-sensitive: a casing difference IS a different address.
		assert!(!addresses_agree(Network::Trc20, "TJRyWwFs9wTFGZg3JbrVriFbNfCug5tDeC", "tjrywwfs9wtfgzg3jbrvrifbnfcug5tdec"));
		assert!(addresses_agree(Network::Trc20, "TJRyWwFs9wTFGZg3JbrVriFbNfCug5tDeC", "TJRyWwFs9wTFGZg3JbrVriFbNfCug5tDeC"));
		// A TON address has two equally valid renderings of the same (workchain, StateInit hash).
		let raw = "0:83dfd552e63729b472fcbcc8c45ebcc6691702558b68ec7527e1ba403a0f31a8";
		assert!(addresses_agree(Network::Ton, raw, "EQCD39VS5jcptHL8vMjEXrzGaRcCVYto7HUn4bpAOg8xqB2N"));
		assert!(!addresses_agree(Network::Ton, raw, "not-an-address"));
	}

	/// A signature that does not recover the stored key must never reach an assembler: the chain
	/// would accept it and credit a different sender. It is also NOT retryable — a retry
	/// reproduces it, and parking the withdrawal is the honest outcome.
	#[test]
	fn a_signature_from_the_wrong_key_is_refused_not_retried() {
		use k256::ecdsa::SigningKey;

		use crate::key_vault::{gen_secp256k1, secp256k1_pubkey};

		let digest = [7u8; 32];
		let signer = gen_secp256k1();
		let bytes = SigningKey::from_slice(&*signer).unwrap().sign_prehash_recoverable(&digest).unwrap().0.to_bytes();
		let result = SignRawPayloadResult {
			r: hex::encode(&bytes[..32]),
			s: hex::encode(&bytes[32..]),
			v: "01".to_owned(),
		};

		assert!(to_chain_signature(Curve::Secp256k1, &result, &secp256k1_pubkey(&signer), &digest).is_ok());
		let err = to_chain_signature(Curve::Secp256k1, &result, &secp256k1_pubkey(&gen_secp256k1()), &digest).expect_err("a foreign public key must be refused");
		assert_eq!(Status::from(err).code(), Code::PermissionDenied);
	}

	/// Ed25519 has no recovery id to fall out of, so the wrong-key check has to be explicit.
	#[test]
	fn an_ed25519_signature_is_verified_against_the_stored_key() {
		use ed25519_dalek::Signer as _;

		use crate::key_vault::{ed25519_pubkey, gen_ed25519};

		let digest = [5u8; 32];
		let seed = gen_ed25519();
		let bytes = ed25519_dalek::SigningKey::from_bytes(&seed).sign(&digest).to_bytes();
		let result = SignRawPayloadResult {
			r: hex::encode(&bytes[..32]),
			s: hex::encode(&bytes[32..]),
			v: String::new(),
		};

		let signed = to_chain_signature(Curve::Ed25519, &result, &ed25519_pubkey(&seed), &digest).expect("the account's own signature verifies");
		let ChainSignature::Ed25519(out) = signed else {
			panic!("ed25519 must yield an Ed25519 signature")
		};
		assert_eq!(out, bytes);

		let err = to_chain_signature(Curve::Ed25519, &result, &ed25519_pubkey(&gen_ed25519()), &digest).expect_err("a foreign public key must be refused");
		assert_eq!(Status::from(err).code(), Code::PermissionDenied);
	}

	/// Turnkey renders r/s as big integers, so a leading zero byte comes back short.
	#[test]
	fn short_scalars_are_left_padded() {
		assert_eq!(scalar32("01").unwrap()[31], 1);
		assert_eq!(scalar32("01").unwrap()[..31], [0u8; 31]);
		assert_eq!(scalar32(&format!("0x{}", hex::encode([9u8; 32]))).unwrap(), [9u8; 32]);
		assert!(scalar32(&hex::encode([9u8; 33])).is_none());
		assert!(scalar32("zz").is_none());
	}

	#[test]
	fn network_wallet_names_are_distinct_per_network_and_stable_per_user() {
		// One wallet per network — not per user — is the entire fix: naming carries no user id.
		assert_ne!(network_wallet_name(Network::Bep20), network_wallet_name(Network::Polygon));
		assert_ne!(network_wallet_name(Network::Bep20), network_wallet_name(Network::Trc20));
		assert_eq!(
			network_wallet_name(Network::Bep20),
			network_wallet_name(Network::Bep20),
			"the same network always names the same shared wallet, for any and every user"
		);
	}
}

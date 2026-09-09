//! The `wallet_secrets` store — the signer's private control plane.
//!
//! Holds the **sealed** chain private keys (output of [`Vault::seal`](crate::key_vault::Vault::seal),
//! `nonce||ciphertext`) plus the public key/address for watch-only indexing and a
//! `key_alg`/`key_version` tag for future rotation. Lives in the signer's OWN
//! database; the hub has no access.
//!
//! KEK-epoch bookkeeping (the stranded-deposit monument): every row carries the
//! `kek_fp` of the KEK that sealed it, the one-row `kek_sentinel` pins the database's
//! epoch for the boot guard ([`kek_guard`](crate::kek_guard)), and a dead row is
//! archived via `superseded_at` so a rotation can mint a replacement — uniqueness
//! holds over the ACTIVE `(user_id, network)` row only.
//!
//! Least privilege: the provisioning path only ever writes a row and reads the
//! address back. The sealed blob is loaded ONLY by [`find_sealed`](WalletSecrets::find_sealed)
//! (the signing path) and the health/backfill probes — never by an ordinary read.

use domain::money::Network;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::SignerError;

/// A new sealed-key row to persist. Borrows its byte payloads so the caller keeps
/// ownership of (and can promptly drop/zeroize) the sensitive material.
pub struct NewSecret<'a> {
	pub id: Uuid,
	pub user_id: Uuid,
	pub network: Network,
	pub public_key: &'a [u8],
	pub address: &'a str,
	pub sealed_key: &'a [u8],
	pub key_alg: &'a str,
	pub key_version: i32,
	/// Fingerprint of the KEK that sealed `sealed_key` — the row's epoch stamp.
	pub kek_fp: &'a [u8],
}

/// A new Turnkey-backed row to persist. There is no sealed blob and no `kek_fp`: the
/// private key was minted inside the custodian's enclave and has never existed in this
/// process, which is the entire point of the migration. `sign_with` is the custodian's own
/// account address — the only handle the signing path needs.
///
/// `derivation_index` is this row's SLIP-10/BIP-32 path component within the network's
/// SHARED wallet (migration 0004) — forensic metadata explaining which path produced this
/// account. Signing never recomputes it: `sign_with` is the only handle the signing path
/// reads.
pub struct NewTurnkeySecret<'a> {
	pub id: Uuid,
	pub user_id: Uuid,
	pub network: Network,
	pub public_key: &'a [u8],
	pub address: &'a str,
	pub sign_with: &'a str,
	pub key_alg: &'a str,
	pub derivation_index: i64,
}

/// A Turnkey row read back for signing: the custodian handle plus the public key the
/// signature is checked against (recovery-id derivation on secp256k1, a strict verify on
/// Ed25519). No secret material — there is none to read.
pub struct TurnkeyKey {
	pub sign_with: String,
	pub public_key: Vec<u8>,
}

/// A sealed row read back for signing. The blob is still ciphertext — opening it
/// needs the [`Vault`](crate::key_vault::Vault) plus this row's `id` (the AAD
/// `wallet_id`).
pub struct SealedSecret {
	pub id: Uuid,
	pub network: Network,
	pub key_alg: String,
	pub key_version: i32,
	pub sealed_key: Vec<u8>,
}

/// One active row's epoch view for the boot backfill and the key-health diagnostics:
/// enough to probe-unseal (`id` is the AAD wallet id) and to report a dead key
/// without exposing anything but watch-only metadata.
pub struct KeyEpochRow {
	pub id: Uuid,
	pub user_id: Uuid,
	pub network: Network,
	pub address: String,
	pub sealed_key: Vec<u8>,
	pub kek_fp: Option<Vec<u8>>,
	/// Rendered by Postgres (`::text`) — carried for operator display only.
	pub created_at: String,
}

/// The persisted KEK sentinel: the epoch fingerprint + the sealed probe blob.
pub struct Sentinel {
	pub kek_fp: Vec<u8>,
	pub sealed_probe: Vec<u8>,
	/// Rendered by Postgres (`::text`) — when this epoch was pinned.
	pub created_at: String,
}

/// `key_version` tags the at-rest SEALING scheme, and a custody-held row has none: the key
/// was never sealed by us. The column is NOT NULL, so those rows carry 0 — "not ours to
/// open" — rather than claiming envelope version 1 and inviting a future opener to try.
const TURNKEY_KEY_VERSION: i32 = 0;

/// Cloneable: `PgPool` is an `Arc` handle, so a clone shares the same pool. The signer
/// hands one to the key backend and keeps one for the KEK-epoch diagnostics.
#[derive(Clone)]
pub struct WalletSecrets {
	pool: PgPool,
}

impl WalletSecrets {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}

	/// Insert a sealed key, idempotent per ACTIVE `(user_id, network)`: a concurrent
	/// racer that inserted first wins and this is a no-op (the caller re-reads the
	/// canonical address). Never overwrites an existing active key.
	pub async fn insert(&self, secret: &NewSecret<'_>) -> Result<(), SignerError> {
		sqlx::query(
			"INSERT INTO wallet_secrets (id, user_id, network, public_key, address, sealed_key, key_alg, key_version, kek_fp) \
			 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
			 ON CONFLICT (user_id, network) WHERE superseded_at IS NULL DO NOTHING",
		)
		.bind(secret.id)
		.bind(secret.user_id)
		.bind(secret.network.as_str())
		.bind(secret.public_key)
		.bind(secret.address)
		.bind(secret.sealed_key)
		.bind(secret.key_alg)
		.bind(secret.key_version)
		.bind(secret.kek_fp)
		.execute(&self.pool)
		.await?;
		Ok(())
	}

	/// Insert a Turnkey-backed row, idempotent per ACTIVE `(user, network)` exactly like
	/// [`insert`](Self::insert). `sealed_key`/`kek_fp` stay NULL — the `wallet_secrets_backend_handle`
	/// CHECK (migration 0003) is what guarantees the row still carries a usable handle, and
	/// `wallet_secrets_turnkey_has_index` (migration 0004) does the same for `derivation_index`.
	pub async fn insert_turnkey(&self, secret: &NewTurnkeySecret<'_>) -> Result<(), SignerError> {
		sqlx::query(
			"INSERT INTO wallet_secrets (id, user_id, network, public_key, address, key_alg, key_version, backend, turnkey_sign_with, derivation_index) \
			 VALUES ($1, $2, $3, $4, $5, $6, $7, 'turnkey', $8, $9) \
			 ON CONFLICT (user_id, network) WHERE superseded_at IS NULL DO NOTHING",
		)
		.bind(secret.id)
		.bind(secret.user_id)
		.bind(secret.network.as_str())
		.bind(secret.public_key)
		.bind(secret.address)
		.bind(secret.key_alg)
		.bind(TURNKEY_KEY_VERSION)
		.bind(secret.sign_with)
		.bind(secret.derivation_index)
		.execute(&self.pool)
		.await?;
		Ok(())
	}

	/// Pull the next value from the shared derivation-index sequence (migration 0004).
	///
	/// The database is the source of uniqueness, not this code: two concurrent callers can
	/// never observe the same value back. Called ONLY on the not-yet-provisioned path in
	/// [`crate::turnkey::TurnkeyBackend::provision`] — an idempotent re-provision of an
	/// existing `(user, network)` returns the stored row before this is ever reached, so a
	/// repeat call never pulls (and never wastes) an index.
	pub async fn next_derivation_index(&self) -> Result<i64, SignerError> {
		let index: i64 = sqlx::query_scalar("SELECT nextval('wallet_secrets_derivation_index_seq')").fetch_one(&self.pool).await?;
		Ok(index)
	}

	/// The Turnkey wallet ID that hosts every user's account for `network`, if that network's
	/// shared wallet has already been created (migration 0004).
	pub async fn network_wallet_id(&self, network: Network) -> Result<Option<String>, SignerError> {
		let wallet_id = sqlx::query_scalar::<_, String>("SELECT wallet_id FROM turnkey_network_wallets WHERE network = $1")
			.bind(network.as_str())
			.fetch_optional(&self.pool)
			.await?;
		Ok(wallet_id)
	}

	/// Register `network`'s shared Turnkey wallet, idempotent per network: a concurrent
	/// racer that registered first wins and this row's INSERT is then a no-op. The caller does
	/// NOT need to re-read the canonical id to stay correct — the account it just created in
	/// ITS OWN `wallet_id` is already live and signable regardless of which wallet ends up
	/// registered here (Turnkey resolves `sign_with` by account address, not by wallet). Losing
	/// this race only means that wallet is never reused for a later user — the same
	/// orphan-tolerant waste the per-user design used before it, bounded to at most one orphan
	/// per network instead of one per user.
	pub async fn insert_network_wallet(&self, network: Network, wallet_id: &str) -> Result<(), SignerError> {
		sqlx::query("INSERT INTO turnkey_network_wallets (network, wallet_id) VALUES ($1, $2) ON CONFLICT (network) DO NOTHING")
			.bind(network.as_str())
			.bind(wallet_id)
			.execute(&self.pool)
			.await?;
		Ok(())
	}

	/// The custodian handle + public key for the active `(user, network)` row — the Turnkey
	/// signing path's only read.
	///
	/// Returns `Ok(Err(backend))` when the row exists but belongs to a different backend, so
	/// the caller fails closed instead of asking Turnkey to sign with a key it never held.
	/// The two-level result keeps "no row at all" (provision it) distinct from "a local row"
	/// (a composition error until phase 3 dispatches per row).
	pub async fn find_turnkey(&self, user_id: Uuid, network: Network) -> Result<Option<Result<TurnkeyKey, String>>, SignerError> {
		let row = sqlx::query_as::<_, (String, Option<String>, Vec<u8>)>(
			"SELECT backend, turnkey_sign_with, public_key FROM wallet_secrets WHERE user_id = $1 AND network = $2 AND superseded_at IS NULL",
		)
		.bind(user_id)
		.bind(network.as_str())
		.fetch_optional(&self.pool)
		.await?;
		Ok(row.map(|(backend, sign_with, public_key)| match sign_with {
			Some(sign_with) if backend == "turnkey" => Ok(TurnkeyKey { sign_with, public_key }),
			_ => Err(backend),
		}))
	}

	/// The watch-only address for the active `(user, network)` key, if provisioned.
	/// Does NOT touch the sealed blob — the ordinary read path.
	pub async fn find_address(&self, user_id: Uuid, network: Network) -> Result<Option<String>, SignerError> {
		let address = sqlx::query_scalar::<_, String>("SELECT address FROM wallet_secrets WHERE user_id = $1 AND network = $2 AND superseded_at IS NULL")
			.bind(user_id)
			.bind(network.as_str())
			.fetch_optional(&self.pool)
			.await?;
		Ok(address)
	}

	/// The watch-only `(address, public_key)` for the active `(user, network)` key, if
	/// provisioned. Public data only — never the sealed blob; used to (re-)derive the
	/// on-chain address.
	pub async fn find_watch(&self, user_id: Uuid, network: Network) -> Result<Option<(String, Vec<u8>)>, SignerError> {
		let row = sqlx::query_as::<_, (String, Vec<u8>)>("SELECT address, public_key FROM wallet_secrets WHERE user_id = $1 AND network = $2 AND superseded_at IS NULL")
			.bind(user_id)
			.bind(network.as_str())
			.fetch_optional(&self.pool)
			.await?;
		Ok(row)
	}

	/// Backfill the stored watch-only address in place — e.g. a placeholder upgraded to the
	/// real derived address once a chain's encoding lands. Never touches the key material.
	pub async fn update_address(&self, user_id: Uuid, network: Network, address: &str) -> Result<(), SignerError> {
		sqlx::query("UPDATE wallet_secrets SET address = $3 WHERE user_id = $1 AND network = $2 AND superseded_at IS NULL")
			.bind(user_id)
			.bind(network.as_str())
			.bind(address)
			.execute(&self.pool)
			.await?;
		Ok(())
	}

	/// Load the SEALED private key for the active `(user, network)` row — the signer-only
	/// path used by signing (and the round-trip test). The blob is still encrypted; the
	/// caller opens it transiently in the [`Vault`](crate::key_vault::Vault) and zeroizes it.
	///
	/// `sealed_key IS NOT NULL` filters out custody-held rows, which have no blob at all
	/// (migration 0003). Without it the `Vec<u8>` decode would fail on a NULL and turn a
	/// perfectly healthy Turnkey row into a repository error.
	pub async fn find_sealed(&self, user_id: Uuid, network: Network) -> Result<Option<SealedSecret>, SignerError> {
		let row = sqlx::query_as::<_, (Uuid, String, String, i32, Vec<u8>)>(
			"SELECT id, network, key_alg, key_version, sealed_key FROM wallet_secrets \
			 WHERE user_id = $1 AND network = $2 AND superseded_at IS NULL AND sealed_key IS NOT NULL",
		)
		.bind(user_id)
		.bind(network.as_str())
		.fetch_optional(&self.pool)
		.await?;
		row.map(|(id, network, key_alg, key_version, sealed_key)| {
			let network = Network::parse(&network).map_err(|e| SignerError::Repository(e.to_string()))?;
			Ok(SealedSecret {
				id,
				network,
				key_alg,
				key_version,
				sealed_key,
			})
		})
		.transpose()
	}

	/// Every ACTIVE **KEK-sealed** row's epoch view — the boot backfill and the key-health walk.
	///
	/// Scoped to `backend='local'` because that is what a KEK epoch is *about*: a custody-held
	/// row has no blob to probe (its `sealed_key` is NULL), so including it would both break the
	/// `Vec<u8>` decode and report a perfectly healthy key as a KEK casualty.
	pub async fn active_epoch_rows(&self) -> Result<Vec<KeyEpochRow>, SignerError> {
		let rows = sqlx::query_as::<_, (Uuid, Uuid, String, String, Vec<u8>, Option<Vec<u8>>, String)>(
			"SELECT id, user_id, network, address, sealed_key, kek_fp, created_at::text FROM wallet_secrets \
			 WHERE superseded_at IS NULL AND backend = 'local' ORDER BY created_at",
		)
		.fetch_all(&self.pool)
		.await?;
		rows.into_iter()
			.map(|(id, user_id, network, address, sealed_key, kek_fp, created_at)| {
				let network = Network::parse(&network).map_err(|e| SignerError::Repository(e.to_string()))?;
				Ok(KeyEpochRow {
					id,
					user_id,
					network,
					address,
					sealed_key,
					kek_fp,
					created_at,
				})
			})
			.collect()
	}

	/// Stamp a row's KEK fingerprint after a successful probe-unseal (boot backfill /
	/// health check healing a pre-epoch row).
	pub async fn stamp_kek_fp(&self, id: Uuid, kek_fp: &[u8]) -> Result<(), SignerError> {
		sqlx::query("UPDATE wallet_secrets SET kek_fp = $2 WHERE id = $1")
			.bind(id)
			.bind(kek_fp)
			.execute(&self.pool)
			.await?;
		Ok(())
	}

	/// Archive the active `(user, network)` row so a rotation can mint a replacement.
	/// Returns whether a row was actually superseded.
	pub async fn supersede(&self, user_id: Uuid, network: Network) -> Result<bool, SignerError> {
		let result = sqlx::query("UPDATE wallet_secrets SET superseded_at = now() WHERE user_id = $1 AND network = $2 AND superseded_at IS NULL")
			.bind(user_id)
			.bind(network.as_str())
			.execute(&self.pool)
			.await?;
		Ok(result.rows_affected() > 0)
	}

	/// The KEK sentinel row, if this database's epoch has been pinned.
	pub async fn sentinel(&self) -> Result<Option<Sentinel>, SignerError> {
		let row = sqlx::query_as::<_, (Vec<u8>, Vec<u8>, String)>("SELECT kek_fp, sealed_probe, created_at::text FROM kek_sentinel")
			.fetch_optional(&self.pool)
			.await?;
		Ok(row.map(|(kek_fp, sealed_probe, created_at)| Sentinel { kek_fp, sealed_probe, created_at }))
	}

	/// Pin this database's KEK epoch (first boot). Race-safe: a concurrent racer's row
	/// wins and this is a no-op — the caller re-reads and verifies either way.
	pub async fn init_sentinel(&self, kek_fp: &[u8], sealed_probe: &[u8]) -> Result<(), SignerError> {
		sqlx::query("INSERT INTO kek_sentinel (kek_fp, sealed_probe) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING")
			.bind(kek_fp)
			.bind(sealed_probe)
			.execute(&self.pool)
			.await?;
		Ok(())
	}
}

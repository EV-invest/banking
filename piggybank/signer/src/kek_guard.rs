//! Boot-time KEK-epoch enforcement — the guard the stranded deposit paid for.
//!
//! A private key sealed under one KEK is unrecoverable under any other, and that
//! loss used to surface only when a sweep or withdrawal finally tried to sign.
//! This module moves the failure to the earliest possible moments:
//!
//!   1. **Sentinel** (whole-database epoch): first boot seals a known plaintext and
//!      pins the KEK fingerprint in `kek_sentinel`; every later boot must match the
//!      fingerprint AND unseal the probe, or [`enforce`] returns an error and the
//!      signer refuses to serve. A signer running with the wrong KEK can only mint
//!      keys that will strand real money — dying at startup is strictly better.
//!   2. **Backfill** (per-row epoch): rows sealed before `kek_fp` existed are
//!      probe-unsealed once and stamped. A row that fails the probe is PROVABLY dead
//!      — logged at ERROR here and reported by the key-health diagnostics — but does
//!      not stop boot: the healthy keys must keep serving sweeps and withdrawals.

use crate::{error::SignerError, key_vault::Vault, provision::chain_of, secrets::WalletSecrets};

/// The boot report: how many active keys were checked and which are provably dead.
#[derive(Debug)]
pub struct KekReport {
	pub active_keys: usize,
	pub dead_keys: usize,
}

/// Verify the database's KEK epoch against the boot KEK (fail-fast), then backfill
/// per-row fingerprints. Call before serving any RPC.
pub async fn enforce(vault: &Vault, secrets: &WalletSecrets) -> color_eyre::Result<KekReport> {
	let fp = vault.fingerprint();

	match secrets.sentinel().await.map_err(boot_err)? {
		None => {
			// First boot on this database: pin the epoch. Race-safe — if a concurrent
			// racer pinned first, the re-read verifies against ITS row.
			let probe = vault.seal_sentinel().map_err(|e| color_eyre::eyre::eyre!("could not seal the KEK sentinel: {e}"))?;
			secrets.init_sentinel(&fp, &probe).await.map_err(boot_err)?;
			let pinned = secrets
				.sentinel()
				.await
				.map_err(boot_err)?
				.ok_or_else(|| color_eyre::eyre::eyre!("KEK sentinel missing immediately after init"))?;
			verify(vault, &fp, &pinned.kek_fp, &pinned.sealed_probe, &pinned.created_at)?;
			tracing::info!(kek_fp = %short_fp(&fp), "KEK sentinel pinned — this database now belongs to this KEK epoch");
		}
		Some(sentinel) => {
			verify(vault, &fp, &sentinel.kek_fp, &sentinel.sealed_probe, &sentinel.created_at)?;
			tracing::info!(kek_fp = %short_fp(&fp), pinned_at = %sentinel.created_at, "KEK sentinel verified");
		}
	}

	// Per-row backfill: probe pre-epoch rows (kek_fp IS NULL) and stamp survivors.
	// A stamped row that mismatches the boot fp cannot exist past the sentinel check
	// unless it was sealed under a different epoch mid-flight — report it, same as a
	// failed probe: provably dead.
	let rows = secrets.active_epoch_rows().await.map_err(boot_err)?;
	let active_keys = rows.len();
	let mut dead_keys = 0usize;
	for row in rows {
		let healthy = match &row.kek_fp {
			Some(stamped) => stamped.as_slice() == fp,
			None => match vault.open(chain_of(row.network), &row.id.to_string(), &row.sealed_key) {
				Ok(_) => {
					secrets.stamp_kek_fp(row.id, &fp).await.map_err(boot_err)?;
					true
				}
				Err(_) => false,
			},
		};
		if !healthy {
			dead_keys += 1;
			tracing::error!(
				wallet_id = %row.id,
				user_id = %row.user_id,
				network = %row.network,
				address = %row.address,
				created_at = %row.created_at,
				"PROVABLY DEAD KEY: sealed under a different KEK — funds on this address cannot be moved; rotate it via RotateAddress"
			);
		}
	}
	if dead_keys > 0 {
		tracing::error!(dead_keys, active_keys, "KEK backfill found dead keys — see GetKeyHealth for the full list");
	}
	Ok(KekReport { active_keys, dead_keys })
}

/// First 8 hex chars — enough to tell epochs apart in logs without dumping hashes.
pub fn short_fp(fp: &[u8]) -> String {
	hex::encode(&fp[..fp.len().min(4)])
}
fn verify(vault: &Vault, boot_fp: &[u8; 32], stored_fp: &[u8], probe: &[u8], pinned_at: &str) -> color_eyre::Result<()> {
	if stored_fp != boot_fp {
		color_eyre::eyre::bail!(
			"WALLET_KEK is NOT the KEK that sealed this database (sentinel pinned {pinned_at}, epoch fp {}, boot KEK fp {}). \
			 Refusing to serve: every key this signer would mint or open under the wrong KEK strands real funds. \
			 Restore the original WALLET_KEK; a lost KEK means the sealed keys are unrecoverable.",
			short_fp(stored_fp),
			short_fp(boot_fp),
		)
	}
	vault.verify_sentinel(probe).map_err(|_| {
		color_eyre::eyre::eyre!(
			"KEK fingerprint matches the sentinel (pinned {pinned_at}) but the sealed probe would not open — \
			 the sentinel row is corrupt. Refusing to serve until an operator investigates."
		)
	})
}

fn boot_err(err: SignerError) -> color_eyre::eyre::Report {
	color_eyre::eyre::eyre!("KEK guard database access failed: {err}")
}

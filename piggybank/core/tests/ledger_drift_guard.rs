//! FB-21: the ledger-derivation drift guard. TB account flags are immutable on first
//! create, and a logical key's `(ledger, code, flags)` is derived fresh on every
//! `ensure`. If that derivation ever changes for an existing key, TB would reject the
//! changed create as a conflict and park every transfer touching it — a silent
//! foot-gun. The guard persists the resolved flags on the id-map row and, on a later
//! ensure, hard-fails loudly the moment the recomputed derivation differs.
//!
//! Real Postgres only (no DB mocks): the drift check fires in the id-map read, before
//! any TigerBeetle call, so a reachable TB replica is not required. Runs when
//! `DATABASE_URL` is set and skips otherwise. A fresh `user_id` keeps runs isolated on
//! shared infra.

use std::sync::Arc;

use domain::{
	balance::{AccountCode, LedgerAccountKey, ServiceId},
	users::UserId,
};
use piggybank_core::ports::ledger::{HoldingScope, Ledger, LedgerError};
use sqlx::PgPool;
use uuid::Uuid;

mod common;
use common::pool;

/// A TB client init that does not require a live replica (no handshake until first
/// call); the drift guard returns before any TB call, so this is never exercised.
fn ledger(pool: PgPool) -> Arc<dyn Ledger> {
	common::ledger_for(&pool)
}

/// An id-map row whose persisted derivation no longer matches what the key derives
/// today must make the next `ensure` fail loudly with a `Conflict`, not park.
#[tokio::test]
async fn a_drifted_derivation_fails_loudly_on_ensure() {
	let Some(pool) = pool().await else {
		eprintln!("DATABASE_URL unset — skipping ledger drift-guard test");
		return;
	};

	// `UserClaim` is credit-normal ⇒ derived flags = DebitsMustNotExceedCredits (2).
	// Seed the id-map row with the WRONG (debit-normal, 4) flags to simulate a drift.
	let key = LedgerAccountKey::UserClaim(UserId::new());
	let logical_key = key.logical_key();
	let id = Uuid::new_v4().as_u128().to_be_bytes();
	sqlx::query("INSERT INTO tb_accounts (logical_key, tb_account_id, ledger, code, network, flags) VALUES ($1, $2, $3, $4, $5, $6)")
		.bind(&logical_key)
		.bind(&id[..])
		.bind(key.ledger().id() as i32)
		.bind(key.account_code().code() as i32)
		.bind(Option::<&str>::None)
		.bind(4_i32)
		.execute(&pool)
		.await
		.expect("seed drifted id-map row");

	let ledger = ledger(pool);
	let result = ledger.ensure_account(&key).await;
	assert!(
		matches!(result, Err(LedgerError::Conflict(_))),
		"a drifted ledger derivation must surface a loud Conflict, got {result:?}"
	);
}

/// A matching persisted derivation must NOT trip the guard: re-ensuring an existing,
/// correctly-recorded key resolves cleanly (the drift check passes before the TB
/// create). This pins the guard to *drift*, not to every existing row.
#[tokio::test]
async fn a_matching_derivation_passes_the_guard() {
	let Some(pool) = pool().await else {
		eprintln!("DATABASE_URL unset — skipping ledger drift-guard test");
		return;
	};

	let key = LedgerAccountKey::UserClaim(UserId::new());
	let logical_key = key.logical_key();
	let id = Uuid::new_v4().as_u128().to_be_bytes();
	// Persist the CORRECT derived flags (2 for a credit-normal claim).
	sqlx::query("INSERT INTO tb_accounts (logical_key, tb_account_id, ledger, code, network, flags) VALUES ($1, $2, $3, $4, $5, $6)")
		.bind(&logical_key)
		.bind(&id[..])
		.bind(key.ledger().id() as i32)
		.bind(key.account_code().code() as i32)
		.bind(Option::<&str>::None)
		.bind(2_i32)
		.execute(&pool)
		.await
		.expect("seed matching id-map row");

	let ledger = ledger(pool);
	// Cannot reach a guaranteed-up TB here, so assert only that the guard itself does
	// not reject — any error must be a TB-side Unavailable, never a derivation Conflict.
	if let Err(LedgerError::Conflict(msg)) = ledger.ensure_account(&key).await {
		panic!("a matching derivation must not trip the drift guard: {msg}");
	}
}

/// The rows #245 retired — `fund` (code 1), `fee` (code 40) and the company stake
/// `shares_company:<svc>` (code 63) — stay in production's `tb_accounts` for good (a
/// TigerBeetle account cannot be deleted), at zero, and no live key names them any more.
/// A scan of the map must step over them by their code, not fail parsing them: the cap
/// table of a product whose company stake once existed (production's `service_arb`) is
/// read through exactly this scan.
#[tokio::test]
async fn the_retired_rows_in_the_map_are_skipped_by_code_not_parsed() {
	let Some(pool) = pool().await else {
		eprintln!("DATABASE_URL unset — skipping ledger drift-guard test");
		return;
	};
	let service = ServiceId::parse(&format!("drift_{}", &Uuid::new_v4().simple().to_string()[..12])).unwrap();
	// The rows as production has them: the two claims credit-normal (flags 2) on the USDT
	// ledger, the company stake debit-normal (flags 4) on the Share ledger.
	for (logical_key, ledger, code, flags) in [
		("fund".to_owned(), 1_i32, AccountCode::RetiredFundClaim, 2_i32),
		("fee".to_owned(), 1, AccountCode::RetiredFeeClaim, 2),
		(format!("shares_company:{service}"), 3, AccountCode::RetiredCompanyStake, 4),
	] {
		let id = Uuid::new_v4().as_u128().to_be_bytes();
		sqlx::query("INSERT INTO tb_accounts (logical_key, tb_account_id, ledger, code, network, flags) VALUES ($1, $2, $3, $4, NULL, $5) ON CONFLICT (logical_key) DO NOTHING")
			.bind(&logical_key)
			.bind(&id[..])
			.bind(ledger)
			.bind(i32::from(code.code()))
			.bind(flags)
			.execute(&pool)
			.await
			.expect("seed the retired id-map row");
		assert!(code.is_retired());
		assert!(LedgerAccountKey::parse_logical_key(&logical_key).is_err(), "{logical_key} names no live account");
	}

	// Only the retired row is on the Share ledger for this product, so the scan reaches no
	// TigerBeetle call: it must return an empty cap table, not a parse error.
	let holdings = ledger(pool)
		.share_holdings(&HoldingScope::Product(service.clone()))
		.await
		.expect("the scan steps over the retired row");
	assert!(holdings.is_empty(), "the retired company stake is not a holder of {service}: {holdings:?}");
}

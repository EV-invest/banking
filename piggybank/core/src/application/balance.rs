//! Balance use cases — seed fund capital, record deposits, read the fund balance.
//!
//! Commands open one Postgres transaction (the ACID point), record intent + an
//! event to the outbox, and `notify` the relay to move money in TigerBeetle
//! afterwards (Write-Last). The query reads live, TigerBeetle-authoritative balances
//! (Read-First).

use domain::{
	architecture::DomainEvent,
	balance::{LedgerAccountKey, LedgerEvent, Party},
	error::DomainError,
	money::{Network, TxRef, Usdt},
};
use sqlx::PgPool;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::{infrastructure::outbox, ports::ledger::Ledger};

/// Per-rail on-chain liquidity (the treasury / Layer 2). TigerBeetle-authoritative.
pub struct RailLiquidity {
	pub network: Network,
	/// Liquid on-chain USDT the fund holds in this rail's custody wallet.
	pub custody: Usdt,
}

/// The treasury picture: per-rail liquidity (Layer 2) and the claims it backs (Layer 1).
/// Under the unified-claim model the invariant is **global** — `total_custody` (the
/// asset side) equals the sum of all claims — so client liabilities are derived as the
/// remainder beyond the fund's own capital and retained fees.
pub struct Treasury {
	/// Layer 2 — per-rail on-chain liquidity (USDT ledger).
	pub rails: Vec<RailLiquidity>,
	/// Mocked bank (USD) liquidity — a separate ledger, not 1:1 with USDT (off-ramp FX).
	pub bank: Usdt,
	/// Sum of per-rail custody — the asset side of the USDT ledger.
	pub total_custody: Usdt,
	/// Layer 1 — the fund's own unallocated capital.
	pub fund_capital: Usdt,
	/// Layer 1 — retained withdrawal-fee revenue.
	pub fee_revenue: Usdt,
	/// Layer 1 — claims owed to users + services (`total_custody − fund_capital −
	/// fee_revenue`, by the global `sum(custody) == sum(claims)` invariant).
	pub held_for_clients: Usdt,
	/// Of `held_for_clients`, the amount reserved by queued/in-flight withdrawals (the
	/// clearing account's pending balance).
	pub reserved_for_withdrawals: Usdt,
}

/// Seed the company's own capital on `network` (`Dr WALLET / Cr FUND`). Admin-gated
/// at the boundary.
pub async fn seed_fund_capital(pool: &PgPool, relay: &Notify, network: Network, amount: Usdt) -> Result<(), DomainError> {
	if amount.is_zero() {
		return Err(DomainError::Validation("seed amount must be positive".into()));
	}
	let mut tx = pool.begin().await.map_err(repo_err)?;
	let aggregate_id = Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("fund:{network}").as_bytes());
	let payload = serde_json::to_string(&LedgerEvent::CapitalSeeded { network, amount }).map_err(|e| DomainError::Repository(e.to_string()))?;
	outbox::insert_event(&mut tx, Uuid::new_v4(), "fund", aggregate_id, LedgerEvent::KIND, &payload, true).await?;
	tx.commit().await.map_err(repo_err)?;
	relay.notify_one();
	Ok(())
}
/// Record an on-chain deposit, **idempotent by `tx_ref`**: the unique gate makes a
/// second record of the same chain tx impossible, so the credit happens at most
/// once even under concurrent recorders. Returns `true` if newly recorded, `false`
/// for a duplicate.
pub async fn record_deposit(pool: &PgPool, relay: &Notify, tx_ref: TxRef, party: Party, network: Network, amount: Usdt) -> Result<bool, DomainError> {
	if amount.is_zero() {
		return Err(DomainError::Validation("deposit amount must be positive".into()));
	}
	let mut tx = pool.begin().await.map_err(repo_err)?;
	let event_id = Uuid::new_v4();
	let inserted = sqlx::query_scalar::<_, String>(
		"INSERT INTO deposits (tx_ref, party_kind, party_id, network, amount, event_id) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (tx_ref) DO NOTHING RETURNING tx_ref",
	)
	.bind(tx_ref.as_str())
	.bind(party.kind_str())
	.bind(party.id_str())
	.bind(network.as_str())
	.bind(amount.base_units().to_string())
	.bind(event_id)
	.fetch_optional(&mut *tx)
	.await
	.map_err(repo_err)?;
	if inserted.is_none() {
		// Already recorded — drop the tx (no-op) and report idempotent success.
		return Ok(false);
	}
	let deposit_id = Uuid::new_v5(&Uuid::NAMESPACE_OID, tx_ref.as_str().as_bytes());
	let payload = serde_json::to_string(&LedgerEvent::Deposited { party, network, amount }).map_err(|e| DomainError::Repository(e.to_string()))?;
	outbox::insert_event(&mut tx, event_id, "deposit", deposit_id, LedgerEvent::KIND, &payload, true).await?;
	tx.commit().await.map_err(repo_err)?;
	relay.notify_one();
	Ok(true)
}
/// The treasury, read live from TigerBeetle (Read-First): per-rail liquidity plus the
/// claims it backs.
pub async fn treasury(ledger: &dyn Ledger) -> Result<Treasury, DomainError> {
	let mut rails = Vec::with_capacity(Network::ALL.len());
	let mut total_custody = Usdt::ZERO;
	for network in Network::ALL {
		let custody = Usdt::from_base_units(ledger.balance(&LedgerAccountKey::CryptoWallet(network)).await?.posted);
		total_custody = total_custody.checked_add(custody).ok_or_else(|| DomainError::Repository("custody total overflow".into()))?;
		rails.push(RailLiquidity { network, custody });
	}
	let bank = Usdt::from_base_units(ledger.balance(&LedgerAccountKey::BankCustody).await?.posted);
	let fund_capital = Usdt::from_base_units(ledger.balance(&LedgerAccountKey::Fund).await?.posted);
	let fee_revenue = Usdt::from_base_units(ledger.balance(&LedgerAccountKey::FeeRevenue).await?.posted);
	let reserved_for_withdrawals = Usdt::from_base_units(ledger.balance(&LedgerAccountKey::WithdrawalClearing).await?.pending);
	// Global invariant sum(custody) == sum(claims): client liabilities are the custody
	// beyond the fund's own capital and retained fees. Saturating — a transient read
	// skew yields 0, never a panic.
	let held_for_clients = total_custody.checked_sub(fund_capital).and_then(|r| r.checked_sub(fee_revenue)).unwrap_or(Usdt::ZERO);
	Ok(Treasury {
		rails,
		bank,
		total_custody,
		fund_capital,
		fee_revenue,
		held_for_clients,
		reserved_for_withdrawals,
	})
}
fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

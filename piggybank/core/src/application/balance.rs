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

/// Per-network slice of the fund's position — every figure TigerBeetle-authoritative.
pub struct NetworkBalance {
	pub network: Network,
	/// Liquid on-chain USDT the fund holds in this network's wallet.
	pub custody: Usdt,
	/// The fund's own unallocated capital on this network.
	pub fund_free: Usdt,
	/// Held for users/services — `custody − fund_free` by the per-network invariant.
	pub allocated: Usdt,
}

pub struct FundBalance {
	pub networks: Vec<NetworkBalance>,
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
/// The fund's balance, per network, read live from TigerBeetle (Read-First).
pub async fn fund_balance(ledger: &dyn Ledger) -> Result<FundBalance, DomainError> {
	let mut networks = Vec::with_capacity(Network::ALL.len());
	for network in Network::ALL {
		let custody = ledger.balance(&LedgerAccountKey::CryptoWallet(network)).await?.posted;
		let fund_free = ledger.balance(&LedgerAccountKey::Fund(network)).await?.posted;
		// sum(custody:N) == sum(claims:N) by construction, so what is held for
		// users/services is exactly custody beyond the fund's own free capital.
		// Saturating: a transient read skew yields 0, never a panic.
		let allocated = custody.checked_sub(fund_free).unwrap_or(Usdt::ZERO);
		networks.push(NetworkBalance {
			network,
			custody,
			fund_free,
			allocated,
		});
	}
	Ok(FundBalance { networks })
}
fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

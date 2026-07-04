//! Balance use cases — seed fund capital, record deposits, read the fund balance.
//!
//! Commands validate and hand the fact to the [`Deposits`] port, whose adapter is
//! its own atomic unit (one Postgres transaction: the gate row + the outbox event),
//! then `notify` the relay to move money in TigerBeetle afterwards (Write-Last).
//! The query reads live, TigerBeetle-authoritative balances (Read-First).

use domain::{
	balance::{LedgerAccountKey, Party},
	error::DomainError,
	money::{Network, TxRef, Usdt},
};
use tokio::sync::Notify;

use crate::ports::{Custody, Deposits, ledger::Ledger};

/// Per-rail on-chain liquidity (the treasury / Layer 2). `custody` is
/// TigerBeetle-authoritative; the funding fields are the operator's chain view,
/// enriched best-effort — `None` when the rail is unconfigured or the read failed.
pub struct RailLiquidity {
	pub network: Network,
	/// Liquid on-chain USDT the fund holds in this rail's custody wallet.
	pub custody: Usdt,
	/// The rail's treasury hot wallet — the operator funds USDT + gas here.
	pub treasury_address: Option<String>,
	/// USDT actually on-chain in the treasury hot wallet.
	pub onchain_usdt: Option<Usdt>,
	/// Native-coin gas balance (BNB/TRX/TON), pre-rendered by the adapter.
	pub onchain_gas: Option<String>,
	/// The rail's sweep gas-station wallet — fund the native coin here (never USDT).
	pub gas_station_address: Option<String>,
	/// The gas station's native-coin balance, pre-rendered by the adapter.
	pub gas_station_gas: Option<String>,
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
pub async fn seed_fund_capital(deposits: &dyn Deposits, relay: &Notify, network: Network, amount: Usdt) -> Result<(), DomainError> {
	if amount.is_zero() {
		return Err(DomainError::Validation("seed amount must be positive".into()));
	}
	deposits.seed_capital(network, amount).await?;
	relay.notify_one();
	Ok(())
}
/// Record an on-chain deposit, **idempotent by `tx_ref`** (see [`Deposits::record`]).
/// Returns `true` if newly recorded, `false` for a duplicate; the relay is nudged
/// only when a new event was committed.
pub async fn record_deposit(deposits: &dyn Deposits, relay: &Notify, tx_ref: TxRef, party: Party, network: Network, amount: Usdt) -> Result<bool, DomainError> {
	if amount.is_zero() {
		return Err(DomainError::Validation("deposit amount must be positive".into()));
	}
	let recorded = deposits.record(tx_ref, party, network, amount).await?;
	if recorded {
		relay.notify_one();
	}
	Ok(recorded)
}
/// The treasury, read live from TigerBeetle (Read-First): per-rail liquidity plus the
/// claims it backs. Each rail is enriched with the custody adapter's funding view
/// (hot-wallet address + real on-chain USDT/gas) **best-effort** — an unwired rail or
/// a chain-RPC failure leaves those fields `None`; the ledger read must never fail
/// because a chain node is down.
pub async fn treasury(ledger: &dyn Ledger, custody: &dyn Custody) -> Result<Treasury, DomainError> {
	let mut rails = Vec::with_capacity(Network::ALL.len());
	let mut total_custody = Usdt::ZERO;
	for network in Network::ALL {
		let rail_custody = Usdt::from_base_units(ledger.balance(&LedgerAccountKey::CryptoWallet(network)).await?.posted);
		total_custody = total_custody.checked_add(rail_custody).ok_or_else(|| DomainError::Repository("custody total overflow".into()))?;
		let funding = custody.treasury_funding(network).await.unwrap_or_else(|err| {
			tracing::debug!(%network, "treasury funding view unavailable: {err}");
			None
		});
		let (treasury_address, onchain_usdt, onchain_gas, gas_station_address, gas_station_gas) = match funding {
			Some(f) => (Some(f.address), f.onchain_usdt, f.onchain_gas, f.gas_station_address, f.gas_station_gas),
			None => (None, None, None, None, None),
		};
		rails.push(RailLiquidity {
			network,
			custody: rail_custody,
			treasury_address,
			onchain_usdt,
			onchain_gas,
			gas_station_address,
			gas_station_gas,
		});
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

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
	users::UserId,
};
use tokio::sync::Notify;

use crate::ports::{Custody, Deposits, custody::InboundTransfer, deposit_addresses::DepositAddresses, ledger::Ledger};

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
/// What a verified arrival turned out to be, once the chain had its say.
pub struct VerifiedArrival {
	pub recorded: bool,
	pub party: Party,
	pub amount: Usdt,
}

/// Record an out-of-band arrival, taking every material fact from the CHAIN.
///
/// This is the operator's only way to write a deposit by hand, and it is deliberately not a
/// way to *state* one. The caller supplies a reference; the amount and the credited party are
/// read back from the transfer that reference names. So the operator surface cannot mint a
/// balance — the worst a bad reference achieves is a refusal.
///
/// Pay retained fee revenue out to an account that can withdraw it
/// (`Dr FEE / Cr <user claim>`).
///
/// This is the last step of the fee plane and the only one that hands the company its own
/// money. It stops one step short of the chain on purpose. `FeeRevenue` is a
/// retained-earnings account: it has no rail, no address, and no confirmation depth, so
/// paying it out directly would mean standing up a second withdrawal pipeline — address
/// validation, rail liquidity, gas runway, broadcast, confirmations, settle — alongside the
/// audited one. Instead the revenue becomes an ordinary claim, and the company withdraws it
/// through exactly the machinery every account holder uses, inheriting every one of those
/// gates for free.
///
/// Read-First on the revenue balance, and it **refuses** rather than parking when short:
/// unlike an investor's withdrawal nobody is waiting on this, and unpaid revenue simply
/// stays retained at no cost. The relay would refuse it anyway — `FeeRevenue` cannot go
/// negative — but failing here means the operator gets a sentence instead of a parked event.
pub async fn pay_fee_revenue(deposits: &dyn Deposits, ledger: &dyn Ledger, relay: &Notify, user: UserId, amount: Usdt) -> Result<Usdt, DomainError> {
	if amount.is_zero() {
		return Err(DomainError::Validation("a fee payout must be positive".into()));
	}
	let available = Usdt::from_base_units(ledger.balance(&LedgerAccountKey::FeeRevenue).await?.available());
	if amount > available {
		return Err(DomainError::Validation(format!(
			"retained fee revenue is {} USDT — settle more fee units before paying out {}",
			available.to_decimal_string(),
			amount.to_decimal_string()
		)));
	}
	deposits.pay_fee_revenue(user, amount).await?;
	relay.notify_one();
	available.checked_sub(amount).ok_or_else(|| DomainError::Repository("fee revenue underflows".into()))
}

/// Read-First against the chain, then the ordinary idempotent `record_deposit`, so a
/// hand-verified arrival and a scanned one collapse onto the same `tx_ref` and one transfer
/// can never be booked twice.
pub async fn record_verified_arrival(
	deposits: &dyn Deposits,
	custody: &dyn Custody,
	addresses: &dyn DepositAddresses,
	relay: &Notify,
	tx_ref: TxRef,
	network: Network,
	expected_amount: Option<Usdt>,
) -> Result<VerifiedArrival, DomainError> {
	let transfer = custody
		.inbound_transfer(network, &tx_ref)
		.await
		.map_err(|e| DomainError::Repository(format!("chain lookup failed: {e}")))?
		.ok_or_else(|| {
			DomainError::Validation(format!(
				"no confirmed {network} USDT transfer matches {} — check the reference, the rail, and that it has enough confirmations",
				tx_ref.as_str()
			))
		})?;
	// An assertion, never an input: it can only cause a refusal. Its job is to turn a
	// reference that points at some OTHER real transfer — a copy-paste from the wrong row —
	// into a loud error instead of a silent credit of the wrong amount.
	if let Some(expected) = expected_amount
		&& expected != transfer.amount
	{
		return Err(DomainError::Validation(format!(
			"the chain reports {} USDT for this reference, not {}",
			transfer.amount.to_decimal_string(),
			expected.to_decimal_string()
		)));
	}
	let party = attribute(custody, addresses, network, &transfer).await?;
	let recorded = record_deposit(deposits, relay, tx_ref, party.clone(), network, transfer.amount).await?;
	Ok(VerifiedArrival {
		recorded,
		party,
		amount: transfer.amount,
	})
}

/// Decide whose money a confirmed transfer is, from its recipient — and refuse anything that
/// is not ours to credit.
///
/// The treasury case carries the one subtlety: the sweep also lands there, moving USDT from a
/// user's own deposit address, and that dollar is already in `wallet:<net>` behind a claim.
/// Crediting it again would invent fund capital and break `sum(custody) == sum(claims)`, so a
/// treasury arrival is only capital when it came from outside every wallet we control.
async fn attribute(custody: &dyn Custody, addresses: &dyn DepositAddresses, network: Network, transfer: &InboundTransfer) -> Result<Party, DomainError> {
	if let Some(user) = addresses.owner_of(network, &transfer.to).await? {
		return Ok(Party::User(user));
	}
	let funding = custody
		.treasury_funding(network)
		.await
		.map_err(|e| DomainError::Repository(format!("treasury address unavailable: {e}")))?;
	let is_treasury = funding.as_ref().is_some_and(|f| f.address.eq_ignore_ascii_case(&transfer.to));
	if !is_treasury {
		return Err(DomainError::Validation(format!(
			"{} received this transfer and it is not one of our {network} addresses",
			transfer.to
		)));
	}
	let gas_station = funding.as_ref().and_then(|f| f.gas_station_address.clone());
	let internal = addresses.owner_of(network, &transfer.from).await?.is_some()
		|| gas_station.is_some_and(|g| g.eq_ignore_ascii_case(&transfer.from))
		|| funding.as_ref().is_some_and(|f| f.address.eq_ignore_ascii_case(&transfer.from));
	if internal {
		return Err(DomainError::Validation(
			"this transfer is the sweep consolidating funds already on the ledger, not new capital".into(),
		));
	}
	Ok(Party::Piggybank)
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

/// What the fund has earned and may pay itself — the read behind the admin payout
/// screen. The mirror of a user's wallet, for the one claim that is the company's own
/// money rather than money it custodies.
pub struct FundRevenue {
	/// Everything the fund has earned and still holds: the `fee` claim's settled
	/// balance. Grows by every retained withdrawal fee and by every settled 2-and-20
	/// management/performance fee; falls only when a payout settles.
	pub earned: Usdt,
	/// Free to pay out right now — `earned − pending_payout`. Read off the same claim
	/// balance as the other two, so the three can never disagree.
	pub available: Usdt,
	/// Locked by payouts already queued or in flight (the clearing reservation).
	pub pending_payout: Usdt,
	/// Where a payout can ship, and how much of it ships without waiting.
	pub rails: Vec<RevenueRail>,
}

/// Per-rail payout options, mirroring a user's `NetworkWithdrawable`. `payable` is the
/// whole available revenue (a request beyond `instant` is accepted and queued until the
/// treasury is topped up); `instant` is what ships without queueing.
pub struct RevenueRail {
	pub network: Network,
	pub payable: Usdt,
	pub instant: Usdt,
	pub minimum: Usdt,
}

/// The fund's earned revenue and the rails it can be paid out on (Read-First).
///
/// `instant` uses the **same** effective liquidity as the dispatch gate —
/// `min(TB rail, on-chain treasury)` — rather than the TB balance alone, because the
/// operator reading this screen is deciding whether money will actually move. The TB
/// `wallet:<net>` balance over-counts: it includes confirmed deposits still sitting on
/// users' derived addresses, which the hot wallet cannot spend. A treasury read failure
/// degrades to the TB view (best-effort, like the treasury screen) — a flaky node
/// must not blank the page.
pub async fn fund_revenue(ledger: &dyn Ledger, custody: &dyn Custody, configured: &[Network]) -> Result<FundRevenue, DomainError> {
	let claim = ledger.balance(&LedgerAccountKey::FeeRevenue).await?;
	let earned = Usdt::from_base_units(claim.posted);
	let available = Usdt::from_base_units(claim.available());
	let pending_payout = Usdt::from_base_units(claim.locked);

	let mut rails = Vec::with_capacity(configured.len());
	for &network in configured {
		let rail = Usdt::from_base_units(ledger.balance(&LedgerAccountKey::CryptoWallet(network)).await?.posted);
		let effective = match custody.treasury_liquidity(network).await {
			Ok(Some(onchain)) => rail.min(onchain),
			Ok(None) => rail,
			Err(err) => {
				tracing::debug!(%network, "treasury liquidity unavailable for the payout view: {err}");
				rail
			}
		};
		rails.push(RevenueRail {
			network,
			payable: available,
			instant: available.min(effective),
			minimum: domain::withdrawals::WithdrawalPolicy::minimum(network),
		});
	}
	Ok(FundRevenue {
		earned,
		available,
		pending_payout,
		rails,
	})
}

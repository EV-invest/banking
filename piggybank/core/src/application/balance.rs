//! Balance use cases — record chain-proven arrivals (a user's deposit, the capital a
//! person seeds the platform with), read the treasury and the fund's revenue.
//!
//! Commands validate and hand the fact to the [`Deposits`] port, whose adapter is
//! its own atomic unit (one Postgres transaction: the gate row + the outbox event),
//! then `notify` the relay to move money in TigerBeetle afterwards (Write-Last).
//! The query reads live, TigerBeetle-authoritative balances (Read-First).
//!
//! Every operator write here is a *verification*, never a statement: the caller names a
//! chain transaction and the amount and the credited party are read back from it. There
//! is no path that credits a claim from a number an operator typed — the last one
//! (`SeedCapital` with a free amount and no dedup key) was removed in issue #234.
//!
//! Nothing here credits a claim nobody holds (#245). USDT that reaches a treasury hot
//! wallet from outside is *somebody's* — the person who sent it — and is booked as their
//! deposit, followed by their subscription into the `fund` allocation
//! ([`seed_fund_capital`]); the retired fund-owned party is never written again.

use domain::{
	balance::{LedgerAccountKey, Party, ServiceId},
	error::DomainError,
	money::{Network, Shares, TxRef, Usdt},
	subscriptions::{Subscription, SubscriptionId},
	users::UserId,
};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::{
	application::{
		funds::{self as funds_app, FundPorts, NavQuote},
		issuance::{self as issuance_app, UnitHolding},
	},
	ports::{
		AllocationRegistry, Custody, Deposits, SubscriptionRepository, UnitIssuanceRepository, custody::InboundTransfer, deposit_addresses::DepositAddresses, ledger::Ledger, nav::NavMarks,
	},
};

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

/// Record an on-chain deposit, **idempotent by `tx_ref`** (see [`Deposits::record`]).
/// Returns `true` if newly recorded, `false` for a duplicate; the relay is nudged
/// only when a new event was committed.
// The retired parties are refused by name until C-4/C-9 remove them from the type.
#[allow(deprecated)]
pub async fn record_deposit(deposits: &dyn Deposits, relay: &Notify, tx_ref: TxRef, party: Party, network: Network, amount: Usdt) -> Result<bool, DomainError> {
	if amount.is_zero() {
		return Err(DomainError::Validation("deposit amount must be positive".into()));
	}
	// `fee` is the fund's EARNINGS claim: it is credited by settling a fee or retaining a
	// withdrawal fee, each of which moves a dollar that is already on the ledger. A deposit
	// credits a claim against NEW custody, so booking one here would invent revenue nobody
	// earned and put `fee` in the deposit history, where every reader expects an arrival.
	// The refusal is narrow on purpose — `Party` names it so payments can spend it, not so
	// anything may pay into it.
	if matches!(party, Party::Revenue) {
		return Err(DomainError::Validation("the fee claim is credited by settling a fee, never by a deposit".into()));
	}
	// The retired `Fund` claim has nobody behind it (#245). Capital is a person's deposit
	// plus their subscription into the `fund` allocation — `seed_fund_capital` — so no
	// caller, however privileged, can put a dollar on a claim without a holder.
	if matches!(party, Party::Piggybank) {
		return Err(DomainError::Validation(
			"the fund's capital is seeded by its depositor — record it with SeedCapital, naming who sent it".into(),
		));
	}
	let recorded = deposits.record(tx_ref, party, network, amount).await?;
	if recorded {
		relay.notify_one();
	}
	Ok(recorded)
}

/// Whose money a confirmed transfer is, decided by the address it landed on.
///
/// An application-layer answer rather than a [`Party`]: the treasury is not a party
/// anyone is credited as — its arrivals are attributed to a person by the operator who
/// knows who sent them (see [`seed_fund_capital`]) or refused (see
/// [`record_verified_arrival`]) — so the type that names it cannot be the type a deposit
/// is recorded against.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Arrival {
	/// Landed on this user's derived deposit address: their deposit.
	User(UserId),
	/// Landed on the rail's treasury hot wallet from outside every wallet we control.
	Treasury,
}

/// What a verified arrival turned out to be, once the chain had its say.
#[derive(Debug)]
pub struct VerifiedArrival {
	pub recorded: bool,
	/// The person whose claim the deposit credits.
	pub user: UserId,
	pub amount: Usdt,
}

/// Record an out-of-band arrival, taking every material fact from the CHAIN.
///
/// This is the operator's general way to write a deposit by hand, and it is deliberately
/// not a way to *state* one. The caller supplies a reference; the amount and the credited
/// party are read back from the transfer that reference names. So the operator surface
/// cannot mint a balance — the worst a bad reference achieves is a refusal.
///
/// Read-First against the chain, then the ordinary idempotent `record_deposit`, so a
/// hand-verified arrival and a scanned one collapse onto the same `tx_ref` and one transfer
/// can never be booked twice.
///
/// A transfer that landed on the TREASURY is refused here: the chain proves the dollar
/// arrived but not whose it is, and this path credits only whom the chain names. The
/// operator who knows the sender attributes it with `SeedCapital` instead.
pub async fn record_verified_arrival(
	deposits: &dyn Deposits,
	custody: &dyn Custody,
	addresses: &dyn DepositAddresses,
	relay: &Notify,
	tx_ref: TxRef,
	network: Network,
	expected_amount: Option<Usdt>,
) -> Result<VerifiedArrival, DomainError> {
	let (arrival, transfer) = verify_arrival(custody, addresses, network, &tx_ref, expected_amount).await?;
	let user = match arrival {
		Arrival::User(user) => user,
		Arrival::Treasury => {
			return Err(DomainError::Validation(format!(
				"{} is the {network} treasury, so the chain cannot say whose deposit this is — attribute it to its sender with SeedCapital",
				transfer.to
			)));
		}
	};
	let recorded = record_deposit(deposits, relay, tx_ref, Party::User(user), network, transfer.amount).await?;
	Ok(VerifiedArrival {
		recorded,
		user,
		amount: transfer.amount,
	})
}

/// The driven ports a seed borrows: the arrival's evidence (chain + address ownership),
/// the deposit gate it is recorded through, the dealing ports the subscription is priced
/// and gated with, and the store it is opened in.
pub struct SeedPorts<'a> {
	pub deposits: &'a dyn Deposits,
	pub custody: &'a dyn Custody,
	pub addresses: &'a dyn DepositAddresses,
	pub subscriptions: &'a dyn SubscriptionRepository,
	pub funds: FundPorts<'a>,
}

/// What a seed did: whether THIS call recorded the arrival (`false` is the idempotent
/// repeat), the amount the chain reported, and the subscription the depositor's units
/// come from — `None` on a repeat that found the mint already there, `Some` on the one
/// that had to open it (see [`seed_fund_capital`]).
#[derive(Debug)]
pub struct SeededCapital {
	pub recorded: bool,
	pub amount: Usdt,
	pub subscription: Option<Subscription>,
}

/// Seed the platform's capital: a chain-proven arrival on the treasury, booked as
/// `depositor`'s deposit and at once subscribed into the `fund` allocation (#245).
///
/// [`record_verified_arrival`]'s evidence with the opposite verdict on where the money
/// landed: the transfer the reference names must have reached the rail's treasury from an
/// external sender. One that landed on a user's deposit address is that user's deposit —
/// booking it under someone else would hand the depositor a dollar owed to that user — so
/// it is refused and pointed at `RecordDeposit`. The sweep is refused by the shared
/// attribution, as everywhere.
///
/// Two ordinary facts, no new kind of money move: the deposit (`Dr wallet:<net> /
/// Cr user:<depositor>`) and a subscription into `fund` at its computed NAV, exactly the
/// legs an investor's subscription posts. The relay drains them in commit order, so the
/// cash leg finds the deposit already credited; the subscription is therefore opened
/// **without** the Read-First balance check an investor's subscribe runs (the deposit is
/// the balance, and it is not on the ledger yet) — TigerBeetle's non-negative flag stays
/// the backstop, and a parked cash leg leaves the money on the depositor's own claim,
/// where every dollar still has a holder.
///
/// Idempotent by `tx_ref`: the deposit gate admits a reference once, and the subscription's
/// id is derived from the same reference, so a double mint is impossible even under a
/// race. A repeat reports `recorded: false` and normally mints nothing — unless the first
/// call recorded the deposit and died before opening the subscription, which is the one
/// state a repeat repairs: the mint is missing under its deterministic id, the deposit is
/// this depositor's, so the subscription is opened now, priced at the current NAV and
/// through the same gates. A reference recorded under another person's name is refused
/// rather than minted to the caller.
///
/// The subscription is priced and gated **before** the deposit is recorded, so a `fund`
/// that is not dealing or a stale price refuses with nothing written. The one gate an
/// investor's subscribe runs that this does not is the catalog's access level: `fund` is
/// hidden from everyone by design, and the person who seeds it becomes its holder by the
/// act itself — the operator's permission to seed is the admission.
pub async fn seed_fund_capital(
	ports: &SeedPorts<'_>,
	depositor: UserId,
	tx_ref: TxRef,
	network: Network,
	expected_amount: Option<Usdt>,
	now_unix: i64,
) -> Result<SeededCapital, DomainError> {
	let (arrival, transfer) = verify_arrival(ports.custody, ports.addresses, network, &tx_ref, expected_amount).await?;
	if let Arrival::User(_) = arrival {
		return Err(DomainError::Validation(format!(
			"{} is a user's deposit address, so this transfer is that user's deposit, not seed capital — record it with RecordDeposit",
			transfer.to
		)));
	}
	let subscription_id = seed_subscription_id(&tx_ref);
	let mut subscription = funds_app::price_fund_seed(&ports.funds, subscription_id, depositor, transfer.amount, now_unix).await?;
	let recorded = record_deposit(ports.deposits, ports.funds.relay, tx_ref.clone(), Party::User(depositor), network, transfer.amount).await?;
	if !recorded {
		// The deposit and the subscription are two commits: a process that died between
		// them left the cash on the depositor's own claim with no units against it. A repeat
		// is where that gets fixed — the id is a function of the reference, so the mint the
		// first call meant to open is the one looked up here, and one exists at most once.
		if ports.subscriptions.find_by_id(subscription_id).await?.is_some() {
			return Ok(SeededCapital {
				recorded: false,
				amount: transfer.amount,
				subscription: None,
			});
		}
		// Only the person the deposit was booked to can be minted against it: opening the
		// subscription under another name would pull that person's own cash into `fund`
		// and leave the first depositor's on their claim.
		if !ports.deposits.list_by_user(depositor).await?.iter().any(|deposit| deposit.tx_ref == tx_ref) {
			return Err(DomainError::Conflict(format!(
				"{} was already recorded as another person's deposit — its fund subscription can only be opened for them",
				tx_ref.as_str()
			)));
		}
		tracing::warn!(tx_ref = %tx_ref.as_str(), %depositor, "seed capital: the deposit was recorded but its fund subscription was never opened — re-opening it");
	}
	ports.subscriptions.open(&mut subscription).await?;
	ports.funds.relay.notify_one();
	Ok(SeededCapital {
		recorded,
		amount: transfer.amount,
		subscription: Some(subscription),
	})
}

/// The seed subscription's id, a function of the chain reference: one transfer, one mint.
fn seed_subscription_id(tx_ref: &TxRef) -> SubscriptionId {
	SubscriptionId::from_raw(Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("seed:{}", tx_ref.as_str()).as_bytes()))
}

/// The chain's account of a reference: the transfer it names and whose money it is.
///
/// One function for both operator write paths so they can never disagree about what counts
/// as proven — the lookup, the optional assertion and the attribution are the whole of the
/// evidence, and a path that skipped any of them would be the very hole this closes.
async fn verify_arrival(
	custody: &dyn Custody,
	addresses: &dyn DepositAddresses,
	network: Network,
	tx_ref: &TxRef,
	expected_amount: Option<Usdt>,
) -> Result<(Arrival, InboundTransfer), DomainError> {
	let transfer = custody
		.inbound_transfer(network, tx_ref)
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
	let arrival = attribute(custody, addresses, network, &transfer).await?;
	Ok((arrival, transfer))
}

/// Decide whose money a confirmed transfer is, from its recipient — and refuse anything that
/// is not ours to credit.
///
/// The treasury case carries the one subtlety: the sweep also lands there, moving USDT from a
/// user's own deposit address, and that dollar is already in `wallet:<net>` behind a claim.
/// Crediting it again would invent custody and break `sum(custody) == sum(claims)`, so a
/// treasury arrival only counts when it came from outside every wallet we control.
async fn attribute(custody: &dyn Custody, addresses: &dyn DepositAddresses, network: Network, transfer: &InboundTransfer) -> Result<Arrival, DomainError> {
	if let Some(user) = addresses.owner_of(network, &transfer.to).await? {
		return Ok(Arrival::User(user));
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
	Ok(Arrival::Treasury)
}

/// The treasury, read live from TigerBeetle (Read-First): per-rail liquidity plus the
/// claims it backs. Each rail is enriched with the custody adapter's funding view
/// (hot-wallet address + real on-chain USDT/gas) **best-effort** — an unwired rail or
/// a chain-RPC failure leaves those fields `None`; the ledger read must never fail
/// because a chain node is down.
// The retired singleton claims are still read for `held_for_clients` until C-5 replaces it.
#[allow(deprecated)]
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

/// The `fee` allocation as its owners read it — the view behind the admin revenue screen
/// (#245). What the platform has earned is not a claim it may pay itself out of: it is an
/// allocation people hold through units, priced at what it holds, and cash leaves it only
/// by a holder's redemption. So the screen shows the allocation: its cash, what is
/// spoken for, its supply and price, and who holds it.
pub struct FeeAllocationView {
	/// The allocation's cash: the `service:fee` claim's settled balance. Grows by every
	/// retained withdrawal fee, every taker fee and every settled fee class; falls by a
	/// holder's redemption or a payment the owners approved out of it.
	pub cash: Usdt,
	/// Cash not yet spoken for — `cash − reserved`. Read off the same claim balance as
	/// the other two, so the three can never disagree.
	pub available: Usdt,
	/// Reserved by approved payments out of the claim that have not settled.
	pub reserved: Usdt,
	/// Units of `fee` outstanding — what the cash and the held fee classes are divided
	/// over.
	pub units_outstanding: Shares,
	/// The allocation's price and the value it prices: cash plus every product's fee
	/// class it holds at that product's NAV (`funds::nav_of`).
	pub quote: NavQuote,
	/// Who holds `fee`, largest first — people, seated by holder grants.
	pub holders: Vec<UnitHolding>,
}

/// The `fee` allocation, Read-First off the ledger: the claim, the supply, the computed
/// price and the cap table, each read once.
///
/// The claim is read on its own rather than taken from the quote's AUM: the AUM sums the
/// held fee classes in, and this screen has to tell cash from units.
pub async fn fee_allocation(allocations: &dyn AllocationRegistry, ledger: &dyn Ledger, nav: &dyn NavMarks, issuances: &dyn UnitIssuanceRepository) -> Result<FeeAllocationView, DomainError> {
	let fee = ServiceId::fee();
	let claim = ledger.balance(&LedgerAccountKey::ServiceClaim(fee.clone())).await?;
	let quote = funds_app::nav_of(nav, ledger, &fee).await?;
	let holders = issuance_app::unit_holders(allocations, ledger, issuances, fee).await?;
	Ok(FeeAllocationView {
		cash: Usdt::from_base_units(claim.posted),
		available: Usdt::from_base_units(claim.available()),
		reserved: Usdt::from_base_units(claim.locked),
		units_outstanding: holders.units_outstanding,
		quote,
		holders: holders.holders,
	})
}

//! `balance` bounded context — the platform's money, and where it sits.
//!
//! The platform ("piggybank") custodies all value; **every unit of it has a holder** — a
//! person, directly through their claim, or through units of an allocation whose holders
//! are people (issue #245). Two views, both authoritative in TigerBeetle (the data
//! plane); this context is the **pure, wasm-safe** model of the chart of accounts and
//! the parties — no I/O, no `tigerbeetle` types leak in.
//!
//! - **Custody / treasury** (debit-normal): where value physically is — a crypto
//!   wallet per [`Network`] (the per-rail liquidity), plus a mocked bank account.
//!   Assets the platform holds. This is the **only** layer where network lives.
//! - **Claims** (credit-normal, **network-agnostic**): whose value it is — a user's
//!   claim (`UserClaim`), an allocation's pooled funds (`ServiceClaim`, the reserved
//!   `fee` and `fund` allocations included), and queued-withdrawal funds
//!   (`WithdrawalClearing`). USDT is one fungible pool, so a user has ONE claim, not
//!   one per chain. There is no claim without an owner: the platform's own capital is
//!   the `fund` allocation's claim, its earnings the `fee` allocation's, and both are
//!   held by people through units.
//!
//! A management/performance fee never touches either layer while it is being charged:
//! it moves *units* on the Share ledger, from the holder's `UserShares` to the product's
//! fee class, `FeeShares` — the units the `fee` allocation holds in that product. Only
//! the periodic bulk settlement of accumulated fee units crosses into cash (today still
//! `Dr ServiceClaim / Cr FeeRevenue`, retargeted to the `fee` allocation's claim by the
//! fee in-kind step of #245).
//!
//! **Retired accounts.** `Fund` (code 1), `FeeRevenue` (40) and `CompanyShares` (63)
//! were claims and holdings with nobody behind them. Their variants, keys and codes
//! stay — `tb_accounts` resolves by them, the outbox and event log carry the parties
//! that map to them, and the data migration debits them — but nothing new is opened on
//! them, and the codes are never reused (TigerBeetle history).
//!
//! The secondary market (the allocation **book**) adds two escrow accounts: `BookShares`
//! holds a holder's units while a sell order rests, `BookCash` holds a user's USDT while a
//! buy order rests. Both are per user, so what a user has committed to the book is a
//! balance the ledger keeps, never a figure Postgres reasons about — and a fill is one
//! linked batch moving units and cash out of the two escrows at once.
//!
//! Two layers, one invariant: **`sum(custody) == sum(claims)`** globally on the USDT
//! ledger. Per-rail backing is a *treasury* concern (a withdrawal on a short rail is
//! queued, not refused), not a ledger one — which is why the invariant is global, not
//! per-network. A deposit is one balanced transfer `Dr WALLET:<net> / Cr <claim>`
//! (textbook Dr Cash / Cr customer-deposit); network rides on the transaction, never
//! on the claim. There is no "external world" account.

use ev::architecture::{DomainEvent, Id};
use serde::{Deserialize, Serialize};

use crate::{
	error::DomainError,
	money::{Network, Usdt},
	users::UserId,
};

/// A service's stable identity — its first-party service-token `sub` (e.g.
/// `"trading"`, `"real-estate"`). Slug-shaped so it is safe in a logical account
/// key and an authorization comparison.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ServiceId(String);

impl ServiceId {
	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		let value = raw.trim();
		if value.is_empty() || value.len() > 64 {
			return Err(DomainError::Validation("service id must be 1..64 chars".into()));
		}
		if !value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
			return Err(DomainError::Validation("service id must be alphanumeric/-/_".into()));
		}
		Ok(Self(value.to_owned()))
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}

	/// The **fee allocation** — the platform's earned money as a product of its own:
	/// the fee class of every product (the units a 2-and-20 charge moves) plus the cash
	/// those units settle into. Hidden, never listed, and held by people through units
	/// like any other allocation (issue #245).
	pub fn fee() -> Self {
		Self(RESERVED_FEE.to_owned())
	}

	/// The **fund allocation** — the platform's own capital as a product of its own:
	/// seed cash arrives as a subscription into it, and the people who put it in hold
	/// its units. Hidden, never listed (issue #245).
	pub fn fund() -> Self {
		Self(RESERVED_FUND.to_owned())
	}

	/// Whether this slug is one the platform reserves for itself ([`Self::fee`],
	/// [`Self::fund`]). An operator cannot register a product under a reserved slug —
	/// the rows exist from migration `0044` — and only a reserved allocation may hold
	/// units of another product ([`crate::issuance::UnitHolder::Allocation`]).
	pub fn is_reserved(&self) -> bool {
		self.0 == RESERVED_FEE || self.0 == RESERVED_FUND
	}
}

/// The reserved slugs, spelled once. Their `service:<slug>` claim keys do not collide
/// with the retired singleton keys `"fee"` and `"fund"` (`tb_accounts` keeps both).
const RESERVED_FEE: &str = "fee";
const RESERVED_FUND: &str = "fund";

impl core::fmt::Display for ServiceId {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.write_str(&self.0)
	}
}

/// Identity of one operator valuation mark (a `fund_valuations` row). Caller-minted,
/// like every id.
pub type ValuationId = Id<ValuationTag>;
/// Phantom tag making [`ValuationId`] a distinct, incompatible identity type.
pub struct ValuationTag;

/// A holder of value in (or a claim against) the fund — the party a deposit credits
/// and a claim belongs to. Tagged for a self-describing JSON shape in event payloads
/// and projections.
///
/// **Every variant must map to a [`LedgerAccountKey`]**, which is what keeps this the one
/// ubiquitous name for "an addressable end of a money move" rather than one vocabulary per
/// feature. Two deliberate absences follow from that rule:
///
/// - **`clearing` gets no variant.** [`LedgerAccountKey::WithdrawalClearing`] is an
///   *in-flight* account owned by a saga, never a party anyone pays or is paid. Giving it
///   a variant would make it addressable as one end of a payment, and a payment into
///   clearing is money parked behind a reservation nobody can complete.
/// - **an external address is not a party.** It has no claim account at all, so it lives
///   in `PaymentDestination` (`domain::payments`) beside this type rather than inside it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Party {
	/// The fund itself — its own unrestricted capital on the retired `Fund` claim.
	///
	/// Retired (#245): the platform's capital is the `fund` allocation
	/// (`Service(ServiceId::fund())`). The variant stays because its serde tag
	/// (`{"kind":"piggybank"}`) sits in the outbox and the event log and in-flight
	/// payments are still open on the `fund` claim — mapping it onto the allocation
	/// here would strand their completion.
	#[deprecated(note = "retired: replay-only, removed after the ownership contract migration (#245)")]
	Piggybank,
	User(UserId),
	/// An allocation's pooled money — a product's, or one of the platform's own
	/// ([`ServiceId::fee`], [`ServiceId::fund`]).
	Service(ServiceId),
	/// The fund's **earned** money on the retired `FeeRevenue` claim.
	///
	/// Retired (#245): earnings are the `fee` allocation (`Service(ServiceId::fee())`).
	/// Kept for the same reason as [`Party::Piggybank`].
	#[deprecated(note = "retired: replay-only, removed after the ownership contract migration (#245)")]
	Revenue,
}

impl Party {
	/// The discriminator stored in an `*_kind` column.
	// The retired kinds are still the persistence vocabulary of the rows that hold them.
	#[allow(deprecated)]
	pub fn kind_str(&self) -> &'static str {
		match self {
			Self::Piggybank => "piggybank",
			Self::User(_) => "user",
			Self::Service(_) => "service",
			Self::Revenue => "revenue",
		}
	}

	/// The identity stored in an `*_id` column (`None` for the retired singleton claims).
	#[allow(deprecated)]
	pub fn id_str(&self) -> Option<String> {
		match self {
			Self::Piggybank | Self::Revenue => None,
			Self::User(id) => Some(id.to_string()),
			Self::Service(id) => Some(id.as_str().to_owned()),
		}
	}

	/// Reconstruct from the `(kind, id)` column pair (persistence adapter). The retired
	/// kinds still read: stored rows and queued events name them.
	#[allow(deprecated)]
	pub fn from_parts(kind: &str, id: Option<&str>) -> Result<Self, DomainError> {
		match (kind, id) {
			("piggybank", _) => Ok(Self::Piggybank),
			("revenue", _) => Ok(Self::Revenue),
			("user", Some(raw)) => {
				let uuid = uuid::Uuid::parse_str(raw).map_err(|_| DomainError::Validation("invalid user party id".into()))?;
				Ok(Self::User(Id::from_raw(uuid)))
			}
			("service", Some(raw)) => Ok(Self::Service(ServiceId::parse(raw)?)),
			_ => Err(DomainError::Validation(format!("invalid party: {kind}"))),
		}
	}

	/// The network-agnostic, credit-normal claim account that holds this party's value.
	/// The relay credits/debits this when moving the party's money; network rides on the
	/// custody side of the transfer. The retired parties keep their retired claims, so a
	/// replayed or in-flight move lands where its counterpart did.
	#[allow(deprecated)]
	pub fn claim_key(&self) -> LedgerAccountKey {
		match self {
			Self::Piggybank => LedgerAccountKey::Fund,
			Self::User(user) => LedgerAccountKey::UserClaim(*user),
			Self::Service(service) => LedgerAccountKey::ServiceClaim(service.clone()),
			Self::Revenue => LedgerAccountKey::FeeRevenue,
		}
	}
}

/// Standalone ledger facts not tied to a Postgres aggregate — the fund's accounts
/// live in TigerBeetle, so a deposit or a capital injection is recorded straight to
/// the outbox for the relay to move. Internally tagged for a self-describing payload.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LedgerEvent {
	/// Value arrived from outside (an on-chain deposit) and was credited to a party.
	/// Ledger: `Dr WALLET:<net> / Cr <party claim>`.
	Deposited { party: Party, network: Network, amount: Usdt },
	/// The company injected its own capital. Ledger: `Dr WALLET:<net> / Cr FUND`.
	///
	/// No producer any more (issue #234 removed the free-amount `SeedCapital`); the variant
	/// stays so historical outbox events and their TigerBeetle transfers can still be read,
	/// replayed and reconciled. New capital arrives as [`Deposited`](Self::Deposited) —
	/// today still with the retired `party: Piggybank`, proven against the chain like any
	/// other arrival; the seed step of #245 makes it a deposit to the depositor and a
	/// subscription into the `fund` allocation.
	CapitalSeeded { network: Network, amount: Usdt },
}

impl DomainEvent for LedgerEvent {
	const KIND: &'static str = "balance";
}

/// A unit of value in TigerBeetle (`ledger`). Only same-ledger accounts transact, so
/// a fund-unit transfer can never touch a cash account — the two planes can't imbalance
/// each other (a stray cross-ledger pairing is a hard TB error, never a silent leak).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ledger {
	/// Canonical 18-dp USDT value ledger.
	Usdt,
	/// Mocked bank (USD) ledger — the real-money seam.
	UsdMock,
	/// Fund units (shares) — the **service currency**. Units float; their cash value is
	/// `units × NAV`, priced off this ledger, not held on it.
	Share,
}

impl Ledger {
	pub const fn id(self) -> u32 {
		match self {
			Self::Usdt => 1,
			Self::UsdMock => 2,
			Self::Share => 3,
		}
	}
}

/// Account type (TigerBeetle `code`).
///
/// Codes are immutable on the accounts that carry them and part of TigerBeetle's
/// history, so a retired code is **reserved forever, never reused**: `1`, `40` and `63`
/// still name the retired accounts until the data migration empties them, and a new
/// account kind takes a fresh number (`retired_codes_are_never_derived_by_a_live_key`
/// guards the derivation).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountCode {
	/// Retired (#245) — reserved, never reuse. The `fund` singleton claim.
	Fund,
	CryptoWallet,
	BankCustody,
	UserClaim,
	ServiceClaim,
	/// Retired (#245) — reserved, never reuse. The `fee` singleton claim.
	FeeRevenue,
	WithdrawalClearing,
	UserShares,
	SharesOutstanding,
	FeeShares,
	/// Retired (#245) — reserved, never reuse. The company's in-kind stake.
	CompanyShares,
	BookShares,
	BookCash,
}

impl AccountCode {
	pub const fn code(self) -> u16 {
		match self {
			Self::Fund => 1,
			Self::CryptoWallet => 10,
			Self::BankCustody => 11,
			Self::UserClaim => 20,
			Self::ServiceClaim => 30,
			Self::FeeRevenue => 40,
			Self::WithdrawalClearing => 50,
			Self::UserShares => 60,
			Self::SharesOutstanding => 61,
			Self::FeeShares => 62,
			Self::CompanyShares => 63,
			Self::BookShares => 64,
			Self::BookCash => 65,
		}
	}
}

/// Which side an account's balance is normal on — drives the non-negative flag and
/// how a posted/pending balance is computed from `debits`/`credits`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Normal {
	/// Assets (custody): balance = `debits − credits`; guard with
	/// `CreditsMustNotExceedDebits`.
	Debit,
	/// Liabilities/equity (claims): balance = `credits − debits`; guard with
	/// `DebitsMustNotExceedCredits`.
	Credit,
}

/// Transfer type (TigerBeetle `code`) — classifies a money movement for audit and
/// reconciliation. Never load-bearing for correctness, only for forensics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferCode {
	SeedCapital,
	Deposit,
	Withdraw,
	WithdrawFee,
	UserAllocate,
	UserRevoke,
	ServiceReserve,
	ServiceSettle,
	ServiceCancel,
	ServiceTransfer,
	Bridge,
	Subscribe,
	Redeem,
	ShareMint,
	ShareBurn,
	FeeClawback,
	FeeSettle,
	/// A [`crate::payments::PaymentOrder`] moving value between two claims — both the
	/// reservation into `clearing` and its settlement out of it.
	///
	/// ONE CODE, NOT ONE PER TIER. The code is forensic, and the tier is already derivable
	/// from the claim pair a transfer names: a credit to `service:<id>` IS the service
	/// tier. Three codes would encode the same fact twice and let the two disagree.
	PaymentTransfer,
	/// An in-kind unit issuance: a mint with no cash leg behind it. Its own code rather
	/// than [`Self::ShareMint`] because the two explain `SharesOutstanding` growth in
	/// opposite ways — a `ShareMint` is always paired with a `Subscribe` cash leg into
	/// the service claim, an issuance never is — and reconciliation reading the Share
	/// ledger alone must be able to tell which mints the fund's cash should account for.
	UnitIssue,
	/// A book order committing what it may spend: units into `BookShares` for a sell,
	/// cash into `BookCash` for a buy.
	BookLock,
	/// The unspent part of a book order handed back on cancel, on an unfilled IOC
	/// remainder, or on the price improvement a buy filled below its limit.
	BookRelease,
	/// One trade's units leg (`Dr UserShares(buyer) / Cr BookShares(seller)`) and cash leg
	/// (`Dr BookCash(buyer) / Cr UserClaim(seller)`) — posted linked, so a fill is
	/// delivery-versus-payment or nothing.
	BookFill,
	/// The taker's fee on a trade, into `FeeRevenue`, in the same linked batch as the fill.
	BookFee,
	/// Part of the company's in-kind stake handed to a named holder: `Dr UserShares /
	/// Cr CompanyShares`, a move *between holders* like a fee clawback in reverse.
	/// `SharesOutstanding` never moves, so it is neither a [`Self::UnitIssue`] (which
	/// grows supply) nor a [`Self::BookFill`] (which is paid for) — reconciliation must
	/// be able to read a company stake shrinking with nothing minted or sold.
	///
	/// Retired with the company holder (#245): no producer; the code stays so posted
	/// transfers and an undrained outbox row keep their meaning.
	#[deprecated(note = "retired: replay-only, removed after the ownership contract migration (#245)")]
	CompanyStakeTransfer,
	/// In-kind units retired: `Dr SharesOutstanding / Cr <holder shares>` — supply
	/// shrinks, no cash moves. The mirror of [`Self::UnitIssue`], and its own code rather
	/// than [`Self::ShareBurn`] for the same reason: a `ShareBurn` is always paired with
	/// a `Redeem` payout out of the service claim, a retirement never is, so
	/// reconciliation reading the Share ledger alone can tell which burns the fund's
	/// cash should account for.
	UnitRetire,
}

impl TransferCode {
	// The retired code is still forensic vocabulary for the transfers that carry it.
	#[allow(deprecated)]
	pub const fn code(self) -> u16 {
		match self {
			Self::SeedCapital => 1,
			Self::Deposit => 2,
			Self::Withdraw => 3,
			Self::WithdrawFee => 4,
			Self::UserAllocate => 10,
			Self::UserRevoke => 11,
			Self::ServiceReserve => 20,
			Self::ServiceSettle => 21,
			Self::ServiceCancel => 22,
			Self::ServiceTransfer => 23,
			Self::Bridge => 30,
			Self::Subscribe => 40,
			Self::Redeem => 41,
			Self::ShareMint => 42,
			Self::ShareBurn => 43,
			Self::FeeClawback => 44,
			Self::FeeSettle => 45,
			Self::PaymentTransfer => 46,
			Self::UnitIssue => 47,
			Self::BookLock => 48,
			Self::BookRelease => 49,
			Self::BookFill => 50,
			Self::BookFee => 51,
			Self::CompanyStakeTransfer => 52,
			Self::UnitRetire => 53,
		}
	}
}

/// The logical identity of a ledger account — resolved to a concrete `u128`
/// TigerBeetle id (minted once, stored in the `tb_accounts` map) by the adapter.
/// Carries everything the adapter needs to *create* the account correctly the first
/// time: `ledger`, `code`, and the non-negative flag (all immutable in TB once set).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LedgerAccountKey {
	/// The fund's own unrestricted capital (credit-normal, network-agnostic claim).
	///
	/// Retired (#245): the platform's capital is the `fund` allocation's
	/// `ServiceClaim`. The key (`"fund"`, code 1) stays resolvable so pending transfers
	/// complete and the data migration can debit it.
	#[deprecated(note = "retired: replay-only, removed after the ownership contract migration (#245)")]
	Fund,
	/// The fund's on-chain custody wallet for a rail (debit-normal asset). The only
	/// network-bearing account — per-rail liquidity (the treasury layer).
	CryptoWallet(Network),
	/// A user's single, network-agnostic claim (credit-normal). One account per user.
	UserClaim(UserId),
	/// An allocation's pooled funds (credit-normal, network-agnostic) — a product's, or
	/// the reserved `fee` / `fund` allocation's (`service:fee`, `service:fund`).
	ServiceClaim(ServiceId),
	/// The fund's retained withdrawal-fee revenue (credit-normal, network-agnostic).
	///
	/// Retired (#245): earnings are the `fee` allocation's `ServiceClaim`. The key
	/// (`"fee"`, code 40) stays resolvable for the same reasons as [`Self::Fund`]; the fee
	/// settlement, the taker fee and the withdrawal fee still credit it until the fee
	/// in-kind step retargets them.
	#[deprecated(note = "retired: replay-only, removed after the ownership contract migration (#245)")]
	FeeRevenue,
	/// Funds reserved for queued/in-flight withdrawals, not yet sent on-chain
	/// (credit-normal, network-agnostic). Decoupled from any rail, so a withdrawal can
	/// be accepted and queued even when the chosen rail is short on liquidity.
	WithdrawalClearing,
	/// The mocked bank custody account (debit-normal, USD ledger).
	BankCustody,
	/// A user's unit holding in a fund (debit-normal, Share ledger). One per
	/// `(service, user)`. Its `credits_must_not_exceed_debits` flag is what makes a
	/// burn (a credit, including a *pending* reserve) that exceeds the user's minted
	/// units (its debits) rejected atomically by TigerBeetle — the over-redeem backstop.
	UserShares(ServiceId, UserId),
	/// A fund's total units in circulation (credit-normal, Share ledger). One per
	/// service. By construction:
	///
	/// `SharesOutstanding(svc) == Σ_user UserShares(svc, user) + Σ_user BookShares(svc, user)
	/// + Σ_{h reserved} shares_holder(svc, h) + CompanyShares(svc)`
	///
	/// where `shares_holder(svc, fee)` is `FeeShares(svc)` (the only allocation holding in
	/// phase 1) and `CompanyShares` is the retired company stake — zero once the data
	/// migration has moved it. Every term is a person's holding or an allocation's whose
	/// holders are people, so the supply always adds up to owners.
	SharesOutstanding(ServiceId),
	/// The product's fee class: the units the `fee` allocation holds in a fund
	/// (debit-normal, Share ledger). One per service — a holder of units exactly like a
	/// user, which is the point: a management or performance fee is charged by moving
	/// units *between holders* (`Dr FeeShares / Cr UserShares`), never by moving cash and
	/// never by minting. Supply is untouched, so NAV per unit does not move and no other
	/// investor pays for the charge; and because no value leaves custody, charging a fee
	/// costs no chain fee at all. The balance is converted to cash in one bulk settlement
	/// per period ([`crate::fees::FeeSettlement`]). It is
	/// [`UnitHolder::Allocation(fee)`](crate::issuance::UnitHolder::Allocation)'s
	/// `shares_key` in every product.
	FeeShares(ServiceId),
	/// The company's own stake in a fund (debit-normal, Share ledger). One per service —
	/// a holder of units exactly like a user or the fee account, minted by an operator's
	/// **in-kind issuance** (`Dr CompanyShares / Cr SharesOutstanding`, no cash leg): the
	/// product is registered against an asset the company already owns, so its share of
	/// the supply was never bought with cash through a subscription. It counts toward the
	/// unit cap and dilutes NAV like any other holding; it has no cost-basis projection,
	/// because there is no investor to report P&L to.
	///
	/// Retired (#245): the company holds nothing; a product's own stake goes to people or
	/// to the `fee` allocation. The key (`shares_company:<svc>`, code 63) stays resolvable
	/// so replayed legs post and the data migration can debit it.
	#[deprecated(note = "retired: replay-only, removed after the ownership contract migration (#245)")]
	CompanyShares(ServiceId),
	/// A user's units committed to resting sell orders on a fund's book (debit-normal,
	/// Share ledger). One per `(service, user)`. A sell order moves its size here
	/// (`Dr BookShares / Cr UserShares` — the holding's non-negative flag refuses an
	/// over-lock atomically), a fill moves units from here to the buyer's holding, a
	/// cancel moves the rest back. The user still owns these units — they count in the
	/// supply invariant and in the position view as "units in orders" — but they cannot
	/// be redeemed or sold twice while they sit here.
	BookShares(ServiceId, UserId),
	/// A user's USDT committed to resting buy orders (credit-normal, USDT ledger, one per
	/// user across every book). A buy order moves its reserve here (`Dr UserClaim / Cr
	/// BookCash` — the claim's non-negative flag refuses an over-lock), a fill pays the
	/// seller out of it, a cancel or a price improvement hands the rest back. A claim like
	/// any other, so `sum(custody) == sum(claims)` is untouched by a lock.
	BookCash(UserId),
}

impl LedgerAccountKey {
	/// Stable string key for the `tb_accounts` id-map (and the idempotent create). The
	/// retired keys keep their strings: the map rows exist and must keep resolving.
	#[allow(deprecated)]
	pub fn logical_key(&self) -> String {
		match self {
			Self::Fund => "fund".to_owned(),
			Self::CryptoWallet(net) => format!("wallet:{net}"),
			Self::UserClaim(user) => format!("user:{user}"),
			Self::ServiceClaim(service) => format!("service:{service}"),
			Self::FeeRevenue => "fee".to_owned(),
			Self::WithdrawalClearing => "clearing".to_owned(),
			Self::BankCustody => "bank".to_owned(),
			Self::UserShares(service, user) => format!("shares:{service}:{user}"),
			Self::SharesOutstanding(service) => format!("shares_outstanding:{service}"),
			Self::FeeShares(service) => format!("shares_fee:{service}"),
			Self::CompanyShares(service) => format!("shares_company:{service}"),
			Self::BookShares(service, user) => format!("book_shares:{service}:{user}"),
			Self::BookCash(user) => format!("book_cash:{user}"),
		}
	}

	#[allow(deprecated)]
	pub fn ledger(&self) -> Ledger {
		match self {
			Self::BankCustody => Ledger::UsdMock,
			Self::UserShares(..) | Self::SharesOutstanding(_) | Self::FeeShares(_) | Self::CompanyShares(_) | Self::BookShares(..) => Ledger::Share,
			_ => Ledger::Usdt,
		}
	}

	#[allow(deprecated)]
	pub fn account_code(&self) -> AccountCode {
		match self {
			Self::Fund => AccountCode::Fund,
			Self::CryptoWallet(_) => AccountCode::CryptoWallet,
			Self::BankCustody => AccountCode::BankCustody,
			Self::UserClaim(_) => AccountCode::UserClaim,
			Self::ServiceClaim(_) => AccountCode::ServiceClaim,
			Self::FeeRevenue => AccountCode::FeeRevenue,
			Self::WithdrawalClearing => AccountCode::WithdrawalClearing,
			Self::UserShares(..) => AccountCode::UserShares,
			Self::SharesOutstanding(_) => AccountCode::SharesOutstanding,
			Self::FeeShares(_) => AccountCode::FeeShares,
			Self::CompanyShares(_) => AccountCode::CompanyShares,
			Self::BookShares(..) => AccountCode::BookShares,
			Self::BookCash(_) => AccountCode::BookCash,
		}
	}

	/// Custody (wallet/bank) and every unit holding — the book's unit escrow included —
	/// are debit-normal; every claim (the book's cash escrow included) and the
	/// units-outstanding contra are credit-normal.
	#[allow(deprecated)]
	pub fn normal(&self) -> Normal {
		match self {
			Self::CryptoWallet(_) | Self::BankCustody | Self::UserShares(..) | Self::FeeShares(_) | Self::CompanyShares(_) | Self::BookShares(..) => Normal::Debit,
			Self::Fund | Self::UserClaim(_) | Self::ServiceClaim(_) | Self::FeeRevenue | Self::WithdrawalClearing | Self::SharesOutstanding(_) | Self::BookCash(_) => Normal::Credit,
		}
	}

	pub fn network(&self) -> Option<Network> {
		match self {
			Self::CryptoWallet(net) => Some(*net),
			_ => None,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::users::UserId;

	#[test]
	fn service_id_validates() {
		assert_eq!(ServiceId::parse(" trading ").unwrap().as_str(), "trading");
		assert!(ServiceId::parse("").is_err());
		assert!(ServiceId::parse("bad space").is_err());
		assert!(ServiceId::parse("real-estate_1").is_ok());
	}

	#[test]
	fn the_reserved_slugs_parse_like_any_other_and_key_their_own_claims() {
		// `fee` and `fund` are ordinary slugs on the wire and in the registry — what sets
		// them apart is the predicate, not the parser.
		assert_eq!(ServiceId::parse("fee").unwrap(), ServiceId::fee());
		assert_eq!(ServiceId::parse("fund").unwrap(), ServiceId::fund());
		assert!(ServiceId::fee().is_reserved());
		assert!(ServiceId::fund().is_reserved());
		assert!(!ServiceId::parse("trading").unwrap().is_reserved());
		assert!(!ServiceId::parse("fees").unwrap().is_reserved(), "reserved is exact, not a prefix");
		// Their claims are `service:<slug>` accounts, distinct from the retired singletons
		// that the same words used to name — both sets stay resolvable in `tb_accounts`.
		assert_eq!(LedgerAccountKey::ServiceClaim(ServiceId::fee()).logical_key(), "service:fee");
		assert_eq!(LedgerAccountKey::ServiceClaim(ServiceId::fund()).logical_key(), "service:fund");
		#[allow(deprecated)]
		{
			assert_ne!(LedgerAccountKey::ServiceClaim(ServiceId::fee()).logical_key(), LedgerAccountKey::FeeRevenue.logical_key());
			assert_ne!(LedgerAccountKey::ServiceClaim(ServiceId::fund()).logical_key(), LedgerAccountKey::Fund.logical_key());
		}
	}

	#[test]
	fn party_round_trips_through_columns() {
		let uid = UserId::new();
		for party in [
			Party::User(uid),
			Party::Service(ServiceId::parse("trading").unwrap()),
			Party::Service(ServiceId::fee()),
			Party::Service(ServiceId::fund()),
		] {
			let back = Party::from_parts(party.kind_str(), party.id_str().as_deref()).unwrap();
			assert_eq!(party, back);
		}
	}

	#[test]
	#[allow(deprecated)]
	fn the_retired_parties_still_read_and_keep_their_claims() {
		// The serde tags `{"kind":"piggybank"}` / `{"kind":"revenue"}` sit in the outbox and
		// the event log, and pending transfers are open on the `fund` and `fee` claims:
		// the retired parties must round-trip and must map to the SAME accounts they
		// always did — not onto the reserved allocations' claims, which would strand an
		// in-flight completion on an account nobody credited.
		for (party, kind, claim, successor) in [
			(Party::Piggybank, "piggybank", LedgerAccountKey::Fund, ServiceId::fund()),
			(Party::Revenue, "revenue", LedgerAccountKey::FeeRevenue, ServiceId::fee()),
		] {
			assert_eq!(party.kind_str(), kind);
			assert_eq!(party.id_str(), None);
			assert_eq!(Party::from_parts(kind, None).unwrap(), party);
			assert_eq!(party.claim_key(), claim);
			assert_ne!(party.claim_key().logical_key(), Party::Service(successor).claim_key().logical_key());
		}
		assert_eq!(serde_json::to_string(&Party::Piggybank).unwrap(), r#"{"kind":"piggybank"}"#);
		assert_eq!(serde_json::from_str::<Party>(r#"{"kind":"revenue"}"#).unwrap(), Party::Revenue);
	}

	#[test]
	fn party_serializes_self_describing() {
		let json = serde_json::to_string(&Party::Service(ServiceId::parse("trading").unwrap())).unwrap();
		assert_eq!(json, r#"{"kind":"service","id":"trading"}"#);
		assert_eq!(serde_json::to_string(&Party::Service(ServiceId::fee())).unwrap(), r#"{"kind":"service","id":"fee"}"#);
	}

	#[test]
	#[allow(deprecated)]
	fn account_keys_and_sides() {
		let uid = UserId::from_raw(uuid::Uuid::nil());
		assert_eq!(LedgerAccountKey::UserClaim(uid).logical_key(), "user:00000000-0000-0000-0000-000000000000");
		assert_eq!(LedgerAccountKey::CryptoWallet(Network::Bep20).logical_key(), "wallet:bep20");
		assert_eq!(LedgerAccountKey::WithdrawalClearing.logical_key(), "clearing");
		assert_eq!(LedgerAccountKey::ServiceClaim(ServiceId::fund()).normal(), Normal::Credit);
		assert_eq!(LedgerAccountKey::CryptoWallet(Network::Ton).normal(), Normal::Debit);
		assert_eq!(LedgerAccountKey::UserClaim(uid).network(), None);
		assert_eq!(LedgerAccountKey::CryptoWallet(Network::Ton).network(), Some(Network::Ton));
		assert_eq!(LedgerAccountKey::BankCustody.ledger(), Ledger::UsdMock);
		assert_eq!(LedgerAccountKey::ServiceClaim(ServiceId::fee()).ledger(), Ledger::Usdt);
		// The retired singletons keep the exact strings, sides and ledgers `tb_accounts`
		// holds for them — a drift here would make the id-map refuse the next ensure.
		assert_eq!(LedgerAccountKey::FeeRevenue.logical_key(), "fee");
		assert_eq!(LedgerAccountKey::Fund.logical_key(), "fund");
		assert_eq!(LedgerAccountKey::Fund.normal(), Normal::Credit);
		assert_eq!(LedgerAccountKey::Fund.ledger(), Ledger::Usdt);
	}

	#[test]
	fn share_keys_live_on_the_share_ledger_with_the_right_sides() {
		let uid = UserId::from_raw(uuid::Uuid::nil());
		let svc = ServiceId::parse("trading").unwrap();
		let user_shares = LedgerAccountKey::UserShares(svc.clone(), uid);
		let outstanding = LedgerAccountKey::SharesOutstanding(svc.clone());
		// Both share accounts MUST be on the Share ledger — a stray Usdt pairing would be
		// a hard TB cross-ledger error, so this guards the units plane from the cash plane.
		assert_eq!(user_shares.ledger(), Ledger::Share);
		assert_eq!(outstanding.ledger(), Ledger::Share);
		// The user's holding is debit-normal (the over-redeem backstop); supply is credit-normal.
		assert_eq!(user_shares.normal(), Normal::Debit);
		assert_eq!(outstanding.normal(), Normal::Credit);
		assert_eq!(user_shares.logical_key(), "shares:trading:00000000-0000-0000-0000-000000000000");
		assert_eq!(outstanding.logical_key(), "shares_outstanding:trading");
		assert_eq!(user_shares.network(), None);
		// A person's holding in a reserved allocation is keyed like any other product's.
		assert_eq!(
			LedgerAccountKey::UserShares(ServiceId::fee(), uid).logical_key(),
			"shares:fee:00000000-0000-0000-0000-000000000000"
		);
	}

	#[test]
	fn the_fee_class_is_a_unit_holder_like_any_other() {
		let fee = LedgerAccountKey::FeeShares(ServiceId::parse("trading").unwrap());
		// Same ledger and same side as a user's holding — a clawback is a plain
		// holder-to-holder unit transfer, so supply (and therefore NAV) never moves.
		assert_eq!(fee.ledger(), Ledger::Share);
		assert_eq!(fee.normal(), Normal::Debit);
		assert_eq!(fee.account_code(), AccountCode::FeeShares);
		assert_eq!(fee.logical_key(), "shares_fee:trading");
		assert_eq!(fee.network(), None);
		// Distinct from the cash-side claim it is eventually settled into.
		assert_ne!(fee.ledger(), LedgerAccountKey::ServiceClaim(ServiceId::fee()).ledger());
	}

	#[test]
	#[allow(deprecated)]
	fn the_retired_company_stake_keeps_its_account() {
		let company = LedgerAccountKey::CompanyShares(ServiceId::parse("service_arb").unwrap());
		// Still a holding on the Share ledger: a replayed mint posts there, and the data
		// migration debits it. Its own account, never aliased onto the fee class or a user's.
		assert_eq!(company.ledger(), Ledger::Share);
		assert_eq!(company.normal(), Normal::Debit);
		assert_eq!(company.account_code(), AccountCode::CompanyShares);
		assert_eq!(company.logical_key(), "shares_company:service_arb");
		assert_eq!(company.network(), None);
		assert_ne!(company.logical_key(), LedgerAccountKey::FeeShares(ServiceId::parse("service_arb").unwrap()).logical_key());
	}

	#[test]
	fn retired_codes_are_never_derived_by_a_live_key() {
		// Codes 1, 40 and 63 belong to the retired accounts and to TigerBeetle's history.
		// Every key a live path can build must derive something else — reusing one would
		// let a new account wear a retired account's forensic identity.
		const RETIRED: [u16; 3] = [AccountCode::Fund.code(), AccountCode::FeeRevenue.code(), AccountCode::CompanyShares.code()];
		assert_eq!(RETIRED, [1, 40, 63]);
		let uid = UserId::from_raw(uuid::Uuid::nil());
		let svc = ServiceId::parse("trading").unwrap();
		let live = [
			LedgerAccountKey::CryptoWallet(Network::Bep20),
			LedgerAccountKey::BankCustody,
			LedgerAccountKey::UserClaim(uid),
			LedgerAccountKey::ServiceClaim(svc.clone()),
			LedgerAccountKey::ServiceClaim(ServiceId::fee()),
			LedgerAccountKey::ServiceClaim(ServiceId::fund()),
			LedgerAccountKey::WithdrawalClearing,
			LedgerAccountKey::UserShares(svc.clone(), uid),
			LedgerAccountKey::UserShares(ServiceId::fee(), uid),
			LedgerAccountKey::SharesOutstanding(svc.clone()),
			LedgerAccountKey::FeeShares(svc.clone()),
			LedgerAccountKey::BookShares(svc, uid),
			LedgerAccountKey::BookCash(uid),
		];
		for key in live {
			assert!(!RETIRED.contains(&key.account_code().code()), "{key:?} derives a retired code");
			assert!(!matches!(key.logical_key().as_str(), "fund" | "fee"), "{key:?} derives a retired logical key");
			assert!(!key.logical_key().starts_with("shares_company:"), "{key:?} derives a retired logical key");
		}
	}

	#[test]
	fn the_book_escrows_sit_beside_the_accounts_they_lock() {
		let uid = UserId::from_raw(uuid::Uuid::nil());
		let svc = ServiceId::parse("service_arb").unwrap();
		let units = LedgerAccountKey::BookShares(svc.clone(), uid);
		// Units in a resting sell are still units: same ledger and side as the holding they
		// left, so the supply invariant keeps summing them and a lock is a plain
		// holder-to-holder transfer the holding's flag can refuse.
		assert_eq!(units.ledger(), Ledger::Share);
		assert_eq!(units.normal(), Normal::Debit);
		assert_eq!(units.account_code(), AccountCode::BookShares);
		assert_eq!(units.logical_key(), "book_shares:service_arb:00000000-0000-0000-0000-000000000000");
		assert_ne!(units.logical_key(), LedgerAccountKey::UserShares(svc, uid).logical_key());
		// Cash in a resting buy is still a claim: same ledger and side as the user's claim,
		// so the global custody-vs-claims invariant does not move when an order is placed.
		let cash = LedgerAccountKey::BookCash(uid);
		assert_eq!(cash.ledger(), Ledger::Usdt);
		assert_eq!(cash.normal(), Normal::Credit);
		assert_eq!(cash.account_code(), AccountCode::BookCash);
		assert_eq!(cash.logical_key(), "book_cash:00000000-0000-0000-0000-000000000000");
		assert_eq!(cash.network(), None);
		assert_ne!(cash.logical_key(), LedgerAccountKey::UserClaim(uid).logical_key());
	}

	#[test]
	// The retired codes are IN the set on purpose: reserved forever, so a newcomer that
	// picks one collides here.
	#[allow(deprecated)]
	fn every_account_and_transfer_code_is_unique() {
		let account_codes = [
			AccountCode::Fund,
			AccountCode::CryptoWallet,
			AccountCode::BankCustody,
			AccountCode::UserClaim,
			AccountCode::ServiceClaim,
			AccountCode::FeeRevenue,
			AccountCode::WithdrawalClearing,
			AccountCode::UserShares,
			AccountCode::SharesOutstanding,
			AccountCode::FeeShares,
			AccountCode::CompanyShares,
			AccountCode::BookShares,
			AccountCode::BookCash,
		]
		.map(AccountCode::code);
		let mut sorted = account_codes;
		sorted.sort_unstable();
		let mut deduped = sorted.to_vec();
		deduped.dedup();
		assert_eq!(deduped.len(), account_codes.len(), "an account code is reused");

		let transfer_codes = [
			TransferCode::SeedCapital,
			TransferCode::Deposit,
			TransferCode::Withdraw,
			TransferCode::WithdrawFee,
			TransferCode::UserAllocate,
			TransferCode::UserRevoke,
			TransferCode::ServiceReserve,
			TransferCode::ServiceSettle,
			TransferCode::ServiceCancel,
			TransferCode::ServiceTransfer,
			TransferCode::Bridge,
			TransferCode::Subscribe,
			TransferCode::Redeem,
			TransferCode::ShareMint,
			TransferCode::ShareBurn,
			TransferCode::FeeClawback,
			TransferCode::FeeSettle,
			TransferCode::PaymentTransfer,
			TransferCode::UnitIssue,
			TransferCode::BookLock,
			TransferCode::BookRelease,
			TransferCode::BookFill,
			TransferCode::BookFee,
			TransferCode::CompanyStakeTransfer,
			TransferCode::UnitRetire,
		]
		.map(TransferCode::code);
		let mut sorted = transfer_codes;
		sorted.sort_unstable();
		let mut deduped = sorted.to_vec();
		deduped.dedup();
		assert_eq!(deduped.len(), transfer_codes.len(), "a transfer code is reused");
	}
}

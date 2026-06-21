//! `balance` bounded context — the fund's money, and where it sits.
//!
//! The fund ("piggybank") holds all value. Two views, both authoritative in
//! TigerBeetle (the data plane); this context is the **pure, wasm-safe** model of
//! the chart of accounts and the parties — no I/O, no `tigerbeetle` types leak in.
//!
//! - **Custody / treasury** (debit-normal): where value physically is — a crypto
//!   wallet per [`Network`] (the per-rail liquidity), plus a mocked bank account.
//!   Assets the fund holds. This is the **only** layer where network lives.
//! - **Claims** (credit-normal, **network-agnostic**): whose value it is — the
//!   fund's own capital (`Fund`), a user's claim (`UserClaim`), a service's funds
//!   (`ServiceClaim`), retained fees (`FeeRevenue`), and queued-withdrawal funds
//!   (`WithdrawalClearing`). USDT is one fungible pool, so a user has ONE claim, not
//!   one per chain.
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
}

impl core::fmt::Display for ServiceId {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.write_str(&self.0)
	}
}

/// A holder of value in (or a claim against) the fund. The owner/sharer roles on an
/// [`Allocation`](crate::allocations::Allocation) are `Party`s. Tagged for a self-
/// describing JSON shape in event payloads and the `sharers` projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Party {
	/// The fund itself — its own unrestricted capital, and the custodian that holds
	/// user/service value. The only owner under which a user may revoke.
	Piggybank,
	User(UserId),
	Service(ServiceId),
}

impl Party {
	/// The discriminator stored in an `*_kind` column.
	pub fn kind_str(&self) -> &'static str {
		match self {
			Self::Piggybank => "piggybank",
			Self::User(_) => "user",
			Self::Service(_) => "service",
		}
	}

	/// The identity stored in an `*_id` column (`None` for the singleton fund).
	pub fn id_str(&self) -> Option<String> {
		match self {
			Self::Piggybank => None,
			Self::User(id) => Some(id.to_string()),
			Self::Service(id) => Some(id.as_str().to_owned()),
		}
	}

	/// Reconstruct from the `(kind, id)` column pair (persistence adapter).
	pub fn from_parts(kind: &str, id: Option<&str>) -> Result<Self, DomainError> {
		match (kind, id) {
			("piggybank", _) => Ok(Self::Piggybank),
			("user", Some(raw)) => {
				let uuid = uuid::Uuid::parse_str(raw).map_err(|_| DomainError::Validation("invalid user party id".into()))?;
				Ok(Self::User(Id::from_raw(uuid)))
			}
			("service", Some(raw)) => Ok(Self::Service(ServiceId::parse(raw)?)),
			_ => Err(DomainError::Validation(format!("invalid party: {kind}"))),
		}
	}

	pub fn is_piggybank(&self) -> bool {
		matches!(self, Self::Piggybank)
	}

	/// The network-agnostic, credit-normal claim account that holds this party's value
	/// (the fund's own capital for `Piggybank`). The relay credits/debits this when
	/// moving the party's money; network rides on the custody side of the transfer.
	pub fn claim_key(&self) -> LedgerAccountKey {
		match self {
			Self::Piggybank => LedgerAccountKey::Fund,
			Self::User(user) => LedgerAccountKey::UserClaim(*user),
			Self::Service(service) => LedgerAccountKey::ServiceClaim(service.clone()),
		}
	}
}

/// Standalone ledger facts not tied to a Postgres aggregate — the fund's accounts
/// live in TigerBeetle, so a deposit or a capital injection is recorded straight to
/// the outbox for the relay to move. Internally tagged for a self-describing payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LedgerEvent {
	/// Value arrived from outside (an on-chain deposit) and was credited to a party.
	/// Ledger: `Dr WALLET:<net> / Cr <party claim>`.
	Deposited { party: Party, network: Network, amount: Usdt },
	/// The company injected its own capital. Ledger: `Dr WALLET:<net> / Cr FUND`.
	CapitalSeeded { network: Network, amount: Usdt },
}

impl DomainEvent for LedgerEvent {
	const KIND: &'static str = "balance";
}

/// A unit of value in TigerBeetle (`ledger`). Only same-ledger accounts transact, so
/// a fund-unit transfer can never touch a cash account — the two planes can't imbalance
/// each other (a stray cross-ledger pairing is a hard TB error, never a silent leak).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountCode {
	Fund,
	CryptoWallet,
	BankCustody,
	UserClaim,
	ServiceClaim,
	FeeRevenue,
	WithdrawalClearing,
	UserShares,
	SharesOutstanding,
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
		}
	}
}

/// Which side an account's balance is normal on — drives the non-negative flag and
/// how a posted/pending balance is computed from `debits`/`credits`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

impl TransferCode {
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
		}
	}
}

/// The logical identity of a ledger account — resolved to a concrete `u128`
/// TigerBeetle id (minted once, stored in the `tb_accounts` map) by the adapter.
/// Carries everything the adapter needs to *create* the account correctly the first
/// time: `ledger`, `code`, and the non-negative flag (all immutable in TB once set).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerAccountKey {
	/// The fund's own unrestricted capital (credit-normal, network-agnostic claim).
	Fund,
	/// The fund's on-chain custody wallet for a rail (debit-normal asset). The only
	/// network-bearing account — per-rail liquidity (the treasury layer).
	CryptoWallet(Network),
	/// A user's single, network-agnostic claim (credit-normal). One account per user.
	UserClaim(UserId),
	/// A service's owned funds (credit-normal, network-agnostic).
	ServiceClaim(ServiceId),
	/// The fund's retained withdrawal-fee revenue (credit-normal, network-agnostic).
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
	/// service. `SharesOutstanding(svc) == Σ_user UserShares(svc, user)` by construction.
	SharesOutstanding(ServiceId),
}

impl LedgerAccountKey {
	/// Stable string key for the `tb_accounts` id-map (and the idempotent create).
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
		}
	}

	pub fn ledger(&self) -> Ledger {
		match self {
			Self::BankCustody => Ledger::UsdMock,
			Self::UserShares(..) | Self::SharesOutstanding(_) => Ledger::Share,
			_ => Ledger::Usdt,
		}
	}

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
		}
	}

	/// Custody (wallet/bank) and a user's unit holding are debit-normal; every claim
	/// and the units-outstanding contra are credit-normal.
	pub fn normal(&self) -> Normal {
		match self {
			Self::CryptoWallet(_) | Self::BankCustody | Self::UserShares(..) => Normal::Debit,
			Self::Fund | Self::UserClaim(_) | Self::ServiceClaim(_) | Self::FeeRevenue | Self::WithdrawalClearing | Self::SharesOutstanding(_) => Normal::Credit,
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
	fn party_round_trips_through_columns() {
		let uid = UserId::new();
		for party in [Party::Piggybank, Party::User(uid), Party::Service(ServiceId::parse("trading").unwrap())] {
			let back = Party::from_parts(party.kind_str(), party.id_str().as_deref()).unwrap();
			assert_eq!(party, back);
		}
	}

	#[test]
	fn party_serializes_self_describing() {
		let json = serde_json::to_string(&Party::Service(ServiceId::parse("trading").unwrap())).unwrap();
		assert_eq!(json, r#"{"kind":"service","id":"trading"}"#);
		assert_eq!(serde_json::to_string(&Party::Piggybank).unwrap(), r#"{"kind":"piggybank"}"#);
	}

	#[test]
	fn account_keys_and_sides() {
		let uid = UserId::from_raw(uuid::Uuid::nil());
		assert_eq!(LedgerAccountKey::UserClaim(uid).logical_key(), "user:00000000-0000-0000-0000-000000000000");
		assert_eq!(LedgerAccountKey::CryptoWallet(Network::Bep20).logical_key(), "wallet:bep20");
		assert_eq!(LedgerAccountKey::FeeRevenue.logical_key(), "fee");
		assert_eq!(LedgerAccountKey::WithdrawalClearing.logical_key(), "clearing");
		assert_eq!(LedgerAccountKey::Fund.normal(), Normal::Credit);
		assert_eq!(LedgerAccountKey::CryptoWallet(Network::Ton).normal(), Normal::Debit);
		assert_eq!(LedgerAccountKey::UserClaim(uid).network(), None);
		assert_eq!(LedgerAccountKey::CryptoWallet(Network::Ton).network(), Some(Network::Ton));
		assert_eq!(LedgerAccountKey::BankCustody.ledger(), Ledger::UsdMock);
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
	}
}

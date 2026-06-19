//! `balance` bounded context — the fund's money, and where it sits.
//!
//! The fund ("piggybank") holds all value. Two views, both authoritative in
//! TigerBeetle (the data plane); this context is the **pure, wasm-safe** model of
//! the chart of accounts and the parties — no I/O, no `tigerbeetle` types leak in.
//!
//! - **Custody** (debit-normal): where value physically is — a crypto wallet per
//!   [`Network`], plus a mocked bank account. Assets the fund holds.
//! - **Claims** (credit-normal): whose value it is — the fund's own capital
//!   (`Fund`), a user's claim (`UserClaim`), a service's funds (`ServiceClaim`).
//!
//! Every account is **per-network** (except the bank mock). That is the load-
//! bearing safety property: a TRC20 claim is backed by TRC20 custody, so a chain-
//! specific withdrawal is always backed and `sum(custody:N) == sum(claims:N)` holds
//! per network. A deposit is one balanced transfer `Dr WALLET:<net> / Cr <claim>`
//! (textbook Dr Cash / Cr customer-deposit); there is no "external world" account.

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

	/// The credit-normal claim account that holds this party's value on `network`
	/// (the fund's own capital for `Piggybank`). The relay credits/debits this when
	/// moving the party's money.
	pub fn claim_key(&self, network: Network) -> LedgerAccountKey {
		match self {
			Self::Piggybank => LedgerAccountKey::Fund(network),
			Self::User(user) => LedgerAccountKey::UserClaim(*user, network),
			Self::Service(service) => LedgerAccountKey::ServiceClaim(service.clone(), network),
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
	/// The company injected its own capital. Ledger: `Dr WALLET:<net> / Cr FUND:<net>`.
	CapitalSeeded { network: Network, amount: Usdt },
}

impl DomainEvent for LedgerEvent {
	const KIND: &'static str = "balance";
}

/// A unit of value in TigerBeetle (`ledger`). Only same-ledger accounts transact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ledger {
	/// Canonical 18-dp USDT value ledger.
	Usdt,
	/// Mocked bank (USD) ledger — the real-money seam.
	UsdMock,
}

impl Ledger {
	pub const fn id(self) -> u32 {
		match self {
			Self::Usdt => 1,
			Self::UsdMock => 2,
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
		}
	}
}

/// The logical identity of a ledger account — resolved to a concrete `u128`
/// TigerBeetle id (minted once, stored in the `tb_accounts` map) by the adapter.
/// Carries everything the adapter needs to *create* the account correctly the first
/// time: `ledger`, `code`, and the non-negative flag (all immutable in TB once set).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerAccountKey {
	/// The fund's own unrestricted capital on a network (credit-normal claim).
	Fund(Network),
	/// The fund's on-chain custody wallet for a network (debit-normal asset).
	CryptoWallet(Network),
	/// A user's claim on a network (credit-normal).
	UserClaim(UserId, Network),
	/// A service's owned funds on a network (credit-normal).
	ServiceClaim(ServiceId, Network),
	/// The fund's retained withdrawal-fee revenue on a network (credit-normal claim).
	FeeRevenue(Network),
	/// The mocked bank custody account (debit-normal, USD ledger).
	BankCustody,
}

impl LedgerAccountKey {
	/// Stable string key for the `tb_accounts` id-map (and the idempotent create).
	pub fn logical_key(&self) -> String {
		match self {
			Self::Fund(net) => format!("fund:{net}"),
			Self::CryptoWallet(net) => format!("wallet:{net}"),
			Self::UserClaim(user, net) => format!("user:{user}:{net}"),
			Self::ServiceClaim(service, net) => format!("service:{service}:{net}"),
			Self::FeeRevenue(net) => format!("fee:{net}"),
			Self::BankCustody => "bank".to_owned(),
		}
	}

	pub fn ledger(&self) -> Ledger {
		match self {
			Self::BankCustody => Ledger::UsdMock,
			_ => Ledger::Usdt,
		}
	}

	pub fn account_code(&self) -> AccountCode {
		match self {
			Self::Fund(_) => AccountCode::Fund,
			Self::CryptoWallet(_) => AccountCode::CryptoWallet,
			Self::BankCustody => AccountCode::BankCustody,
			Self::UserClaim(..) => AccountCode::UserClaim,
			Self::ServiceClaim(..) => AccountCode::ServiceClaim,
			Self::FeeRevenue(_) => AccountCode::FeeRevenue,
		}
	}

	/// Custody (wallet/bank) is debit-normal; every claim is credit-normal.
	pub fn normal(&self) -> Normal {
		match self {
			Self::CryptoWallet(_) | Self::BankCustody => Normal::Debit,
			Self::Fund(_) | Self::UserClaim(..) | Self::ServiceClaim(..) | Self::FeeRevenue(_) => Normal::Credit,
		}
	}

	pub fn network(&self) -> Option<Network> {
		match self {
			Self::Fund(net) | Self::CryptoWallet(net) | Self::UserClaim(_, net) | Self::ServiceClaim(_, net) | Self::FeeRevenue(net) => Some(*net),
			Self::BankCustody => None,
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
		assert_eq!(LedgerAccountKey::UserClaim(uid, Network::Trc20).logical_key(), "user:00000000-0000-0000-0000-000000000000:trc20");
		assert_eq!(LedgerAccountKey::CryptoWallet(Network::Bep20).logical_key(), "wallet:bep20");
		assert_eq!(LedgerAccountKey::Fund(Network::Ton).normal(), Normal::Credit);
		assert_eq!(LedgerAccountKey::CryptoWallet(Network::Ton).normal(), Normal::Debit);
		assert_eq!(LedgerAccountKey::BankCustody.ledger(), Ledger::UsdMock);
		assert_eq!(LedgerAccountKey::Fund(Network::Bep20).ledger(), Ledger::Usdt);
	}
}

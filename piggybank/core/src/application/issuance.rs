//! Unit issuance use cases — an operator minting fund units **in kind**.
//!
//! The one supply path beside [`subscribe`](super::funds::subscribe), and deliberately
//! shaped like it: the registry is resolved first, the mint is priced at a fresh NAV,
//! the cap is checked against what the ledger says is issued, and the relay posts the
//! units after the control-plane commit. What differs is what is *absent* — no cash
//! leg, no claim read, no access gate — because the operator is not an investor
//! spending a balance: they are recording that a holder owns a share of an asset the
//! product was registered against.
//!
//! Admin-only at the boundary (`Permission::AllocationManage`). The allocation must be
//! registered but may be in any state: a product sized and seeded before it opens is the
//! normal case, and a closed one may still need its cap table corrected.
//!
//! [`transfer_company_stake`] is the way back out of `CompanyShares`: the same record
//! and the same gates, minus the cap (supply does not move) and plus a Read-First on
//! what the company actually holds.

use domain::{
	balance::{LedgerAccountKey, ServiceId},
	error::DomainError,
	issuance::{IdempotencyKey, IssuanceSource, UnitHolder, UnitIssuance, UnitIssuanceId},
	money::{Shares, Usdt},
	users::UserId,
};

use crate::{
	application::{
		allocations as allocations_app,
		funds::{self as funds_app, FundPorts},
	},
	ports::{
		UnitIssuanceRepository, UserRepository,
		allocations::AllocationRegistry,
		issuance::{IssueOutcome, UnitIssuanceRecord},
		ledger::Ledger,
	},
};

/// One issuance as the operator asked for it. `cost_basis: None` prices it as
/// `units × NAV` at the dealing mark; `Some` is taken as given, zero included.
pub struct IssueUnitsRequest {
	pub service: ServiceId,
	pub holder: UnitHolder,
	pub units: Shares,
	pub cost_basis: Option<Usdt>,
	pub idempotency_key: IdempotencyKey,
}

/// One hand-over of the company's stake as the operator asked for it. `cost_basis:
/// None` prices it as `units × NAV` at the dealing mark, like a mint.
pub struct TransferCompanyStakeRequest {
	pub service: ServiceId,
	pub user: UserId,
	pub units: Shares,
	pub cost_basis: Option<Usdt>,
	pub idempotency_key: IdempotencyKey,
}

/// A fund's issued supply broken down by who holds it. `investor_units` is derived
/// (`outstanding − company − fee`) rather than summed over holders: the ledger keeps
/// one account per investor and reading them all to answer a three-line summary would
/// be a scan the invariant already makes unnecessary.
///
/// `queued_units` is the one figure not read from the ledger: mints recorded but not yet
/// posted by the relay. The settled supply is what `ensure_capacity` reads, so an
/// operator pinning the cap to it while this is non-zero pins it below where the supply
/// is about to land.
pub struct UnitHoldersView {
	pub service: ServiceId,
	pub units_outstanding: Shares,
	pub company_units: Shares,
	pub fee_units: Shares,
	pub investor_units: Shares,
	pub queued_units: Shares,
}

/// Mint `request.units` of `request.service` to `request.holder` with no cash leg.
///
/// Idempotent by `(service, idempotency_key)`: a repeat returns the issuance the key
/// already names — the same row, one mint — provided it asks for the same holder and
/// units. A repeat that names the same key for a *different* request is a
/// [`DomainError::Conflict`]: an admin console retrying a timed-out call is the case
/// the key exists for, and a key reused for a new mint is the mistake it must catch.
///
/// Gates, in order: the key is read before anything is priced (a retry must succeed
/// even if the mark has since gone stale); the allocation must be registered, in any
/// state; a user holder must exist (units minted to a UUID nobody can sign in as are
/// units nobody can redeem); the NAV must be fresh, because it is recorded on the row
/// and blended into the holder's high-water mark; and the cap must hold
/// ([`Allocation::ensure_capacity`](domain::allocations::Allocation::ensure_capacity)),
/// with the same issuance-gate-not-invariant caveats as a subscription. An operator
/// sizing a product below what they mean to issue raises the cap first.
pub async fn issue_units(
	ports: &FundPorts<'_>,
	issuances: &dyn UnitIssuanceRepository,
	users: &dyn UserRepository,
	request: IssueUnitsRequest,
	now_unix: i64,
) -> Result<UnitIssuanceRecord, DomainError> {
	let identity = RequestIdentity {
		service: &request.service,
		holder: request.holder,
		source: IssuanceSource::Mint,
		units: request.units,
		idempotency_key: &request.idempotency_key,
	};
	if let Some(existing) = issuances.find_by_key(&request.service, &request.idempotency_key).await? {
		return identity.same_request_or_conflict(existing);
	}
	let allocation = allocations_app::get(ports.allocations, &request.service).await?;
	if let UnitHolder::User(user) = request.holder {
		require_user(users, user).await?;
	}
	let price = funds_app::dealing_nav(ports.nav, &request.service, now_unix).await?;
	allocation.ensure_capacity(funds_app::issued_units(ports.ledger, &request.service).await?, request.units)?;
	let issuance = UnitIssuance::issue(
		UnitIssuanceId::new(),
		request.service.clone(),
		request.holder,
		request.units,
		price,
		request.cost_basis,
		request.idempotency_key.clone(),
	)?;
	record(ports, issuances, issuance, &identity).await
}

/// Hand `request.units` of the company's stake in `request.service` to `request.user`:
/// `Dr UserShares / Cr CompanyShares` by the relay, supply untouched. This is how the
/// 80 % seeded to the company under [`issue_units`] reaches the person it was always
/// meant for — minting them a second copy would inflate supply, and the company cannot
/// sell on the book (it has no user, so no orders and no escrow to lock; nothing of the
/// company's is ever committed to the book, so `available()` is its whole holding).
///
/// Same record, same key contract ([`issue_units`]) — a repeat with the same key,
/// user and units returns the row; the same key for a different request, a mint under
/// that key included, is a [`DomainError::Conflict`]. Gates, in order: the key is read
/// first; the allocation must be registered, in any state (the recipient's access is
/// not consulted — an operator command, like a mint); the user must exist; the NAV must
/// be fresh (recorded on the row, blended into the recipient's high-water mark, and the
/// default basis is `units × NAV`); and the company must hold at least `units` on the
/// ledger (Read-First; a shortfall is a [`DomainError::Validation`]). No cap check:
/// nothing is minted. TigerBeetle's non-negative flag on `CompanyShares` is the
/// backstop under the read — a race that overdraws it parks the event.
pub async fn transfer_company_stake(
	ports: &FundPorts<'_>,
	issuances: &dyn UnitIssuanceRepository,
	users: &dyn UserRepository,
	request: TransferCompanyStakeRequest,
	now_unix: i64,
) -> Result<UnitIssuanceRecord, DomainError> {
	let identity = RequestIdentity {
		service: &request.service,
		holder: UnitHolder::User(request.user),
		source: IssuanceSource::Company,
		units: request.units,
		idempotency_key: &request.idempotency_key,
	};
	if let Some(existing) = issuances.find_by_key(&request.service, &request.idempotency_key).await? {
		return identity.same_request_or_conflict(existing);
	}
	allocations_app::get(ports.allocations, &request.service).await?;
	require_user(users, request.user).await?;
	let price = funds_app::dealing_nav(ports.nav, &request.service, now_unix).await?;
	let company = Shares::from_base_units(ports.ledger.balance(&LedgerAccountKey::CompanyShares(request.service.clone())).await?.available());
	if company < request.units {
		return Err(DomainError::Validation(format!(
			"the company holds {} units of '{}', cannot transfer {}",
			company.to_decimal_string(),
			request.service,
			request.units.to_decimal_string()
		)));
	}
	let issuance = UnitIssuance::transfer_company_stake(
		UnitIssuanceId::new(),
		request.service.clone(),
		request.user,
		request.units,
		price,
		request.cost_basis,
		request.idempotency_key.clone(),
	)?;
	record(ports, issuances, issuance, &identity).await
}

/// Units minted to a UUID nobody can sign in as are units nobody can redeem.
async fn require_user(users: &dyn UserRepository, user: UserId) -> Result<(), DomainError> {
	if users.find_by_id(user).await?.is_none() {
		return Err(DomainError::NotFound {
			entity: "user",
			id: user.to_string(),
		});
	}
	Ok(())
}

async fn record(ports: &FundPorts<'_>, issuances: &dyn UnitIssuanceRepository, mut issuance: UnitIssuance, identity: &RequestIdentity<'_>) -> Result<UnitIssuanceRecord, DomainError> {
	match issuances.issue(&mut issuance).await? {
		IssueOutcome::Recorded(record) => {
			ports.relay.notify_one();
			Ok(record)
		}
		// Lost the race to a concurrent send of the same request: the winner's row is
		// the answer, subject to the same parameter check as a sequential repeat.
		IssueOutcome::Existing(existing) => identity.same_request_or_conflict(existing),
	}
}

/// What makes two requests under one key "the same": the facts the operator chose,
/// not the ones the hub computed (NAV, a defaulted basis).
struct RequestIdentity<'a> {
	service: &'a ServiceId,
	holder: UnitHolder,
	source: IssuanceSource,
	units: Shares,
	idempotency_key: &'a IdempotencyKey,
}

impl RequestIdentity<'_> {
	fn same_request_or_conflict(&self, existing: UnitIssuanceRecord) -> Result<UnitIssuanceRecord, DomainError> {
		if existing.issuance.matches_request(self.holder, self.source, self.units) {
			Ok(existing)
		} else {
			Err(DomainError::Conflict(format!(
				"idempotency key '{}' already names a different issuance on '{}'",
				self.idempotency_key.as_str(),
				self.service
			)))
		}
	}
}

/// The settled supply of `service` by holder class, plus the mints still in flight.
/// Gated on the allocation existing, like the NAV view: a cap table for a product no
/// registry entry backs is a cap table for a fund that does not exist.
pub async fn unit_holders(allocations: &dyn AllocationRegistry, ledger: &dyn Ledger, issuances: &dyn UnitIssuanceRepository, service: ServiceId) -> Result<UnitHoldersView, DomainError> {
	allocations_app::get(allocations, &service).await?;
	let outstanding = posted_units(ledger, &LedgerAccountKey::SharesOutstanding(service.clone())).await?;
	let company = posted_units(ledger, &LedgerAccountKey::CompanyShares(service.clone())).await?;
	let fee = posted_units(ledger, &LedgerAccountKey::FeeShares(service.clone())).await?;
	let queued = issuances.queued_mint_units(&service).await?;
	// Saturating rather than checked: the invariant makes a negative remainder impossible,
	// and a scan of three accounts that are read at three instants must not fail a
	// read-only view over a mint landing between two of them.
	let investor = outstanding.checked_sub(company).and_then(|rest| rest.checked_sub(fee)).unwrap_or(Shares::ZERO);
	Ok(UnitHoldersView {
		service,
		units_outstanding: outstanding,
		company_units: company,
		fee_units: fee,
		investor_units: investor,
		queued_units: queued,
	})
}

async fn posted_units(ledger: &dyn Ledger, key: &LedgerAccountKey) -> Result<Shares, DomainError> {
	Ok(Shares::from_base_units(ledger.balance(key).await?.posted))
}

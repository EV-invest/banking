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

use domain::{
	balance::{LedgerAccountKey, ServiceId},
	error::DomainError,
	issuance::{IdempotencyKey, UnitHolder, UnitIssuance, UnitIssuanceId},
	money::{Shares, Usdt},
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

/// A fund's issued supply broken down by who holds it. `investor_units` is derived
/// (`outstanding − company − fee`) rather than summed over holders: the ledger keeps
/// one account per investor and reading them all to answer a three-line summary would
/// be a scan the invariant already makes unnecessary.
pub struct UnitHoldersView {
	pub service: ServiceId,
	pub units_outstanding: Shares,
	pub company_units: Shares,
	pub fee_units: Shares,
	pub investor_units: Shares,
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
	if let Some(existing) = issuances.find_by_key(&request.service, &request.idempotency_key).await? {
		return same_request_or_conflict(existing, &request);
	}
	let allocation = allocations_app::get(ports.allocations, &request.service).await?;
	if let UnitHolder::User(user) = request.holder
		&& users.find_by_id(user).await?.is_none()
	{
		return Err(DomainError::NotFound {
			entity: "user",
			id: user.to_string(),
		});
	}
	let price = funds_app::dealing_nav(ports.nav, &request.service, now_unix).await?;
	allocation.ensure_capacity(funds_app::issued_units(ports.ledger, &request.service).await?, request.units)?;
	let mut issuance = UnitIssuance::issue(
		UnitIssuanceId::new(),
		request.service.clone(),
		request.holder,
		request.units,
		price,
		request.cost_basis,
		request.idempotency_key.clone(),
	)?;
	match issuances.issue(&mut issuance).await? {
		IssueOutcome::Recorded(record) => {
			ports.relay.notify_one();
			Ok(record)
		}
		// Lost the race to a concurrent send of the same request: the winner's row is
		// the answer, subject to the same parameter check as a sequential repeat.
		IssueOutcome::Existing(existing) => same_request_or_conflict(existing, &request),
	}
}

fn same_request_or_conflict(existing: UnitIssuanceRecord, request: &IssueUnitsRequest) -> Result<UnitIssuanceRecord, DomainError> {
	if existing.issuance.matches_request(request.holder, request.units) {
		Ok(existing)
	} else {
		Err(DomainError::Conflict(format!(
			"idempotency key '{}' already names a different issuance on '{}'",
			request.idempotency_key.as_str(),
			request.service
		)))
	}
}

/// The settled supply of `service` by holder class. Gated on the allocation existing,
/// like the NAV view: a cap table for a product no registry entry backs is a cap table
/// for a fund that does not exist.
pub async fn unit_holders(allocations: &dyn AllocationRegistry, ledger: &dyn Ledger, service: ServiceId) -> Result<UnitHoldersView, DomainError> {
	allocations_app::get(allocations, &service).await?;
	let outstanding = posted_units(ledger, &LedgerAccountKey::SharesOutstanding(service.clone())).await?;
	let company = posted_units(ledger, &LedgerAccountKey::CompanyShares(service.clone())).await?;
	let fee = posted_units(ledger, &LedgerAccountKey::FeeShares(service.clone())).await?;
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
	})
}

async fn posted_units(ledger: &dyn Ledger, key: &LedgerAccountKey) -> Result<Shares, DomainError> {
	Ok(Shares::from_base_units(ledger.balance(key).await?.posted))
}

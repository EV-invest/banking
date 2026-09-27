//! The one-off ownership data migration (#245, phase 1, step C-6): what is still on the
//! retired singleton claims moves onto the reserved allocations, and the people the
//! owners named become those allocations' first holders.
//!
//! Before it, the platform's capital sits on `fund` (code 1) and its earnings on `fee`
//! (code 40) — claims with nobody behind them — and a product's fee class
//! (`FeeShares(svc)`) is a holding of the `fee` allocation that nobody holds units of.
//! After it, every unit of value is at a holder (the invariant every step of #245 is
//! held to): `service:fund` and `service:fee` carry the cash, `SharesOutstanding(fund)`
//! and `SharesOutstanding(fee)` equal the units the named holders were minted, and the
//! two claims the migration emptied stay at zero until the contract step removes them.
//!
//! The migration runs as a **command of the `piggybank` binary**, not a SQL script:
//! its movements are TigerBeetle transfers and the units it mints need rows and
//! projections. Two things make it safe to run exactly once and harmless to run again:
//!
//! * **Every id is deterministic** — v5 of a spelled-out key under one namespace — so a
//!   second run recomputes the same transfer ids and issuance keys, finds them, and
//!   reports "already applied" instead of moving anything.
//! * **One linked chain per allocation.** The cash hand-over off the retired claim and
//!   every holder's mint land together or not at all, so there is no instant at which
//!   the allocation holds cash while its units are half issued.
//!
//! What it writes past the outbox, and why that is admissible once: the mints are posted
//! here, directly through the [`Ledger`] port, and their `unit_issuances` rows are
//! written already `applied` with the holders' projections
//! ([`UnitIssuanceRepository::record_applied`]) — the relay is never involved. The
//! outbox exists so a request handler can commit and let money follow; this command is
//! run by an operator, after a dry run, with a snapshot before and after, and its whole
//! point is a linked chain the relay does not offer for a mint. The rows carry the same
//! ids the relay would have stamped ([`issuance_mint_id`]), so nothing downstream can
//! tell them apart.
//!
//! The ledger is written first, the rows second. A crash between the two leaves the
//! ledger applied and the rows missing; the next run sees the transfers, reads each
//! mint's amount back from the ledger and writes the rows from that — never from what a
//! re-run would compute, because fees may have accrued in between. There is no rollback
//! after the chain lands, which is why the plan is printed, confirmed and reconciled.

use std::{collections::HashSet, fmt};

use domain::{
	balance::{LedgerAccountKey, ServiceId, TransferCode},
	error::DomainError,
	issuance::{IdempotencyKey, IssuanceSource, UnitHolder, UnitIssuance, UnitIssuanceId},
	money::{Nav, Shares, Usdt},
	users::UserId,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
	application::{
		funds as funds_app,
		issuance::{self as issuance_app, UnitHolding},
		ownership::{self as ownership_app, AllocationOwnership},
	},
	infrastructure::relay::issuance_mint_id,
	ports::{
		AllocationRegistry, NavMarks, UnitIssuanceRepository, UserRepository,
		ledger::{HoldingScope, Ledger, LedgerTransfer},
	},
};

/// A whole allocation, in basis points: every holder table must add up to exactly this.
pub const TOTAL_BPS: u32 = 10_000;

/// The driven ports the migration reads and writes through.
pub struct MigrationPorts<'a> {
	pub ledger: &'a dyn Ledger,
	pub allocations: &'a dyn AllocationRegistry,
	pub users: &'a dyn UserRepository,
	pub nav: &'a dyn NavMarks,
	pub issuances: &'a dyn UnitIssuanceRepository,
}

/// One line of the owners' holder table: a person and their share of an allocation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HolderShare {
	pub user_id: UserId,
	pub share_bps: u32,
}

/// The owners' input: who holds `fee` and who holds `fund`, each table summing to
/// [`TOTAL_BPS`]. Parsed from the `holders.json` the operator passes; refused as a whole
/// before anything is read if either table is malformed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HolderTable {
	pub fee: Vec<HolderShare>,
	pub fund: Vec<HolderShare>,
}

impl HolderTable {
	/// `{"fee": [{"user_id": "<uuid>", "share_bps": 8000}, …], "fund": […]}`, validated.
	pub fn parse_json(raw: &str) -> Result<Self, DomainError> {
		let table: Self = serde_json::from_str(raw).map_err(|err| DomainError::Validation(format!("holders.json: {err}")))?;
		table.validate()?;
		Ok(table)
	}

	/// Each table names at least one person, every share is positive, the shares add up
	/// to [`TOTAL_BPS`], and nobody is listed twice.
	pub fn validate(&self) -> Result<(), DomainError> {
		for (name, rows) in [("fund", &self.fund), ("fee", &self.fee)] {
			if rows.is_empty() {
				return Err(DomainError::Validation(format!(
					"holders.json: the '{name}' table is empty — an allocation with value must have a holder"
				)));
			}
			let mut seen = HashSet::with_capacity(rows.len());
			let mut total: u32 = 0;
			for row in rows {
				if row.share_bps == 0 {
					return Err(DomainError::Validation(format!("holders.json: '{name}' lists {} with a zero share", row.user_id)));
				}
				if !seen.insert(row.user_id) {
					return Err(DomainError::Validation(format!("holders.json: '{name}' lists {} twice", row.user_id)));
				}
				total = total
					.checked_add(row.share_bps)
					.ok_or_else(|| DomainError::Validation(format!("holders.json: the '{name}' shares overflow")))?;
			}
			if total != TOTAL_BPS {
				return Err(DomainError::Validation(format!(
					"holders.json: the '{name}' shares add up to {total} bps, not {TOTAL_BPS} — the whole allocation must be assigned"
				)));
			}
		}
		Ok(())
	}

	fn for_allocation(&self, service: &ServiceId) -> &[HolderShare] {
		if *service == ServiceId::fund() { &self.fund } else { &self.fee }
	}
}

/// The owners' decision on a product's retired company stake (`CompanyShares(svc)`,
/// code 63), asked for only when one is not zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompanyStake {
	/// Leave it: the migration refuses to run while any stake is non-zero, naming it.
	Keep,
	/// Burn it: `Dr SharesOutstanding(svc) / Cr CompanyShares(svc)`, no cash, the
	/// product's supply shrinking by exactly the stake.
	Retire,
}

impl CompanyStake {
	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"keep" => Ok(Self::Keep),
			"retire" => Ok(Self::Retire),
			other => Err(DomainError::Validation(format!("--company must be 'keep' or 'retire', got '{other}'"))),
		}
	}
}

/// Where an allocation's step stands before the run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepStatus {
	/// Not on the ledger yet: the chain will be posted.
	Pending,
	/// Every mint is on the ledger; only rows may still be missing.
	AlreadyApplied,
	/// Nothing stands behind the allocation — no cash, no product units — so there is
	/// nothing to move and nothing to mint.
	Nothing,
}

/// One holder's mint: the share they were named, the units it comes to, and the ids the
/// row and the transfer carry.
#[derive(Clone, Debug)]
pub struct Grant {
	pub user: UserId,
	pub share_bps: u32,
	pub units: Shares,
	pub issuance_id: UnitIssuanceId,
	pub idempotency_key: IdempotencyKey,
	pub transfer_id: u128,
	/// The `unit_issuances` row already stands.
	pub row_present: bool,
	/// The mint is already on the ledger.
	pub posted: bool,
}

/// A product's fee class as the `fee` allocation holds it, priced at the product's
/// dealing NAV — the non-cash part of what stands behind `fee`.
#[derive(Clone, Debug)]
pub struct PricedHolding {
	pub product: ServiceId,
	pub units: Shares,
	pub nav: Nav,
	pub value: Usdt,
}

/// One reserved allocation's step: what moves off its retired claim, what it is worth
/// once that has landed, and who is minted what.
#[derive(Clone, Debug)]
pub struct AllocationStep {
	pub service: ServiceId,
	pub retired_key: LedgerAccountKey,
	/// The retired claim's posted balance — what the cash leg moves.
	pub retired: Usdt,
	/// `service:<svc>` before the move.
	pub claim: Usdt,
	pub holdings: Vec<PricedHolding>,
	/// `retired + claim + Σ holdings` — what the minted units will stand for.
	pub value: Usdt,
	/// The price the units are minted at: the seed NAV, because nothing is outstanding.
	pub nav: Nav,
	pub grants: Vec<Grant>,
	pub status: StepStatus,
	/// The cash leg's transfer id.
	pub claim_transfer_id: u128,
}

impl AllocationStep {
	fn legs(&self, reference: u128) -> Vec<LedgerTransfer> {
		let mut legs = Vec::with_capacity(self.grants.len() + 1);
		if !self.retired.is_zero() {
			legs.push(LedgerTransfer {
				id: self.claim_transfer_id,
				debit: self.retired_key.clone(),
				credit: LedgerAccountKey::ServiceClaim(self.service.clone()),
				amount: self.retired.base_units(),
				code: TransferCode::OwnershipMigrate,
				reference,
			});
		}
		for grant in &self.grants {
			legs.push(LedgerTransfer {
				id: grant.transfer_id,
				debit: LedgerAccountKey::UserShares(self.service.clone(), grant.user),
				credit: LedgerAccountKey::SharesOutstanding(self.service.clone()),
				amount: grant.units.base_units(),
				code: TransferCode::UnitIssue,
				reference: grant.issuance_id.raw().as_u128(),
			});
		}
		legs
	}

	fn rows_missing(&self) -> usize {
		self.grants.iter().filter(|grant| !grant.row_present).count()
	}
}

/// A product's retired company stake and what the run does with it.
#[derive(Clone, Debug)]
pub struct CompanyStep {
	pub product: ServiceId,
	pub units: Shares,
	pub transfer_id: u128,
	/// The burn is already on the ledger (the stake was retired by an earlier run).
	pub retired_earlier: bool,
}

/// The whole plan, as printed to the operator and as executed.
#[derive(Clone, Debug)]
pub struct MigrationPlan {
	pub fund: AllocationStep,
	pub fee: AllocationStep,
	/// Every product whose company stake the run retires (`--company retire`), or has
	/// retired. Empty under `keep`, which refuses a non-zero stake before the plan exists.
	pub company: Vec<CompanyStep>,
}

impl MigrationPlan {
	/// Whether a run would write nothing: every step applied (rows included) or empty,
	/// every stake already retired.
	pub fn is_noop(&self) -> bool {
		[&self.fund, &self.fee].iter().all(|step| step.status != StepStatus::Pending && step.rows_missing() == 0) && self.company.iter().all(|c| c.retired_earlier)
	}
}

/// What one allocation's step did.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StepOutcome {
	pub chain_posted: bool,
	pub rows_written: usize,
	pub rows_already_present: usize,
}

/// What the run did and what it reads back afterwards.
#[derive(Clone, Debug)]
pub struct RunReport {
	pub fund: StepOutcome,
	pub fee: StepOutcome,
	pub company_retired: Vec<ServiceId>,
	pub after: AfterRun,
}

/// The ledger after the run: the retired claims (both must be zero) and each reserved
/// allocation's ownership picture with its price.
#[derive(Clone, Debug)]
pub struct AfterRun {
	pub retired_fund: Usdt,
	pub retired_fee_revenue: Usdt,
	pub fund: AllocationOwnership,
	pub fund_nav: Nav,
	pub fee: AllocationOwnership,
	pub fee_nav: Nav,
}

impl AfterRun {
	/// What the migration itself promised and the ledger must now show: the retired
	/// claims empty, every unit of both reserved allocations at a holder, and nothing on
	/// either that nobody holds. Empty means the run kept its promise; the cash invariant
	/// and the other allocations are the reconciliation's to check.
	pub fn findings(&self) -> Vec<String> {
		let mut findings = Vec::new();
		if !self.retired_fund.is_zero() {
			findings.push(format!("the retired fund claim (code 1) still holds {} USDT", self.retired_fund.to_decimal_string()));
		}
		if !self.retired_fee_revenue.is_zero() {
			findings.push(format!("the retired fee claim (code 40) still holds {} USDT", self.retired_fee_revenue.to_decimal_string()));
		}
		for picture in [&self.fund, &self.fee] {
			if !picture.units_reconcile() {
				findings.push(format!(
					"'{}': units outstanding {} != held {} (supply drift)",
					picture.service,
					picture.units_outstanding.to_decimal_string(),
					picture.held_units().to_decimal_string()
				));
			}
			if picture.is_unheld() {
				findings.push(format!("'{}' holds value while nobody holds units of it", picture.service));
			}
		}
		findings
	}
}

/// The namespace every migration id is minted under. Fixed for good: the ids are what a
/// second run recognises its own work by.
fn namespace() -> Uuid {
	Uuid::new_v5(&Uuid::NAMESPACE_OID, b"piggybank:migrate-ownership")
}

fn migration_id(key: &str) -> Uuid {
	Uuid::new_v5(&namespace(), key.as_bytes())
}

/// The idempotency key of one holder's mint — what the `unit_issuances` row is found by.
pub fn grant_key(service: &ServiceId, user: UserId) -> IdempotencyKey {
	// `migrate-ownership:` + a slug of at most 4 + a UUID: well inside the 64-char cap.
	IdempotencyKey::parse(&format!("migrate-ownership:{service}:{user}")).expect("a reserved slug and a UUID fit an idempotency key")
}

/// Read everything, move nothing: the plan a dry run prints and a run executes.
///
/// Refused, in this order, before any of it: a malformed holder table; a holder that is
/// not an active person; a reserved allocation the registry does not know (migration
/// `0044` not applied); in-flight pendings on a retired account (a legacy transfer still
/// completing — wait); a pending step on an allocation that already has units
/// outstanding (someone was seated before the migration, so the seed price would be
/// wrong — the owners decide); a stale product mark behind a fee class (post a
/// valuation first); a non-zero company stake under `keep`; and a holder table that
/// differs from the one an earlier run applied (some mints exist, some do not).
pub async fn plan(ports: &MigrationPorts<'_>, table: &HolderTable, company: CompanyStake, now_unix: i64) -> Result<MigrationPlan, DomainError> {
	table.validate()?;
	for row in table.fund.iter().chain(&table.fee) {
		issuance_app::require_holder(ports.allocations, ports.users, &UnitHolder::User(row.user_id)).await?;
	}
	// The retired keys are exactly what this migration empties: it is their one
	// remaining producer of debits.
	#[allow(deprecated)]
	let fund = plan_allocation(ports, table, ServiceId::fund(), LedgerAccountKey::Fund, now_unix).await?;
	#[allow(deprecated)]
	let fee = plan_allocation(ports, table, ServiceId::fee(), LedgerAccountKey::FeeRevenue, now_unix).await?;
	let company = plan_company_stakes(ports, company).await?;
	Ok(MigrationPlan { fund, fee, company })
}

async fn plan_allocation(ports: &MigrationPorts<'_>, table: &HolderTable, service: ServiceId, retired_key: LedgerAccountKey, now_unix: i64) -> Result<AllocationStep, DomainError> {
	// The registry row is what the mints' `holder_service` and the treasury's listing
	// hang off; without it the allocation does not exist to the platform.
	if ports.allocations.find(&service).await?.is_none() {
		return Err(DomainError::Precondition(format!(
			"the '{service}' allocation is not registered — migration 0044 (ownership expand) has not been applied"
		)));
	}
	let retired_balance = ports.ledger.balance(&retired_key).await?;
	if retired_balance.pending != 0 || retired_balance.locked != 0 {
		return Err(DomainError::Precondition(format!(
			"'{}' has in-flight pending transfers (pending {}, locked {}) — a legacy withdrawal or payout is still completing; wait for the outbox to drain and retry",
			retired_key.logical_key(),
			retired_balance.pending,
			retired_balance.locked
		)));
	}
	let retired = Usdt::from_base_units(retired_balance.posted);
	let claim = Usdt::from_base_units(ports.ledger.balance(&LedgerAccountKey::ServiceClaim(service.clone())).await?.posted);
	let mut holdings = Vec::new();
	let mut value = retired.checked_add(claim).ok_or_else(|| overflow(&service))?;
	for (key, units) in ports.ledger.share_holdings(&HoldingScope::Holder(UnitHolder::Allocation(service.clone()))).await? {
		if units == 0 {
			continue;
		}
		let Some((product, _)) = UnitHolder::of_holding(&key) else { continue };
		let nav = funds_app::dealing_nav(ports.nav, ports.ledger, &product, now_unix).await.map_err(|err| {
			DomainError::Precondition(format!(
				"'{service}' holds the fee class of '{product}', which cannot be priced: {err} — post a valuation for it first"
			))
		})?;
		let units = Shares::from_base_units(units);
		let holding_value = nav.value(units)?;
		value = value.checked_add(holding_value).ok_or_else(|| overflow(&service))?;
		holdings.push(PricedHolding {
			product,
			units,
			nav,
			value: holding_value,
		});
	}
	holdings.sort_by(|a, b| a.product.as_str().cmp(b.product.as_str()));

	let shares = table.for_allocation(&service);
	let mut grants = Vec::with_capacity(shares.len());
	let mut assigned = Usdt::ZERO;
	for (index, share) in shares.iter().enumerate() {
		let issuance_id = UnitIssuanceId::from_raw(migration_id(&format!("{service}:{}", share.user_id)));
		let idempotency_key = grant_key(&service, share.user_id);
		let transfer_id = issuance_mint_id(issuance_id.raw());
		let row = ports.issuances.find_by_key(&service, &idempotency_key).await?;
		let posted = ports.ledger.transfer_amount(transfer_id).await?;
		let units = match (&row, posted) {
			// Applied and recorded: the row is the record, and it must be the ledger's.
			(Some(record), Some(amount)) => {
				let units = Shares::from_base_units(amount);
				if !record.issuance.matches_request(&UnitHolder::User(share.user_id), IssuanceSource::Mint, units) {
					return Err(DomainError::Conflict(format!(
						"'{service}': the row under '{}' does not match the mint on the ledger ({} units) — inspect before rerunning",
						idempotency_key.as_str(),
						units.to_decimal_string()
					)));
				}
				units
			}
			// Applied on the ledger, the row lost to a crash: the ledger says how much.
			(None, Some(amount)) => Shares::from_base_units(amount),
			// A row with no mint behind it cannot be this migration's: it writes the row
			// only after the chain has landed.
			(Some(_), None) => {
				return Err(DomainError::Conflict(format!(
					"'{service}': a unit_issuances row already stands under '{}' but its mint is not on the ledger — not this migration's row; inspect before rerunning",
					idempotency_key.as_str()
				)));
			}
			// Not applied: this holder's share of the value, floored; the last holder
			// listed takes the rounding remainder, so the units add up to the value
			// exactly and NAV is the seed price to the base unit.
			(None, None) => {
				let part = if index + 1 == shares.len() {
					value.checked_sub(assigned).ok_or_else(|| overflow(&service))?
				} else {
					value.scale(share.share_bps.into(), TOTAL_BPS.into())?
				};
				assigned = assigned.checked_add(part).ok_or_else(|| overflow(&service))?;
				Shares::from_cash(part, Nav::SEED)?
			}
		};
		grants.push(Grant {
			user: share.user_id,
			share_bps: share.share_bps,
			units,
			issuance_id,
			idempotency_key,
			transfer_id,
			row_present: row.is_some(),
			posted: posted.is_some(),
		});
	}

	let posted = grants.iter().filter(|grant| grant.posted).count();
	let status = if posted == grants.len() {
		StepStatus::AlreadyApplied
	} else if posted != 0 {
		return Err(DomainError::Conflict(format!(
			"'{service}': {posted} of {} mints are already on the ledger — the holder table differs from the one an earlier run applied; rerun with that table",
			grants.len()
		)));
	} else if value.is_zero() {
		StepStatus::Nothing
	} else {
		StepStatus::Pending
	};
	if status == StepStatus::Pending {
		let outstanding = Shares::from_base_units(ports.ledger.balance(&LedgerAccountKey::SharesOutstanding(service.clone())).await?.posted);
		if !outstanding.is_zero() {
			return Err(DomainError::Precondition(format!(
				"'{service}' already has {} units outstanding and this migration has not run — someone was seated before it, so the seed price no longer holds; the owners decide how to proceed",
				outstanding.to_decimal_string()
			)));
		}
		// The rounding above cannot lose a base unit, and a mint of zero is not a mint.
		if let Some(grant) = grants.iter().find(|grant| grant.units.is_zero()) {
			return Err(DomainError::Validation(format!(
				"'{service}': {}'s share of {} USDT rounds to zero units — too little value to split this way",
				grant.user,
				value.to_decimal_string()
			)));
		}
	}
	if status != StepStatus::Pending {
		// Nothing minted (or minted earlier): the figures are for the report only.
		grants.retain(|grant| grant.posted);
	}
	// The cash leg is the first leg of the chain, and the chain is idempotent on it: its
	// id names the holder table it was posted with, so the chain and the mints stand or
	// fall together and a chain for one table can never be mistaken for another's.
	let roster: Vec<String> = shares.iter().map(|share| share.user_id.to_string()).collect();
	Ok(AllocationStep {
		claim_transfer_id: migration_id(&format!("{service}:claim:{}", roster.join(","))).as_u128(),
		service,
		retired_key,
		retired,
		claim,
		holdings,
		value,
		nav: Nav::SEED,
		grants,
		status,
	})
}

// The retired company stake is what this step reads and burns.
#[allow(deprecated)]
async fn plan_company_stakes(ports: &MigrationPorts<'_>, decision: CompanyStake) -> Result<Vec<CompanyStep>, DomainError> {
	let mut steps = Vec::new();
	let mut products: Vec<ServiceId> = ports
		.allocations
		.list_all()
		.await?
		.into_iter()
		.map(|a| a.service().clone())
		.filter(|s| !s.is_reserved())
		.collect();
	products.sort_by(|a, b| a.as_str().cmp(b.as_str()));
	for product in products {
		let key = LedgerAccountKey::CompanyShares(product.clone());
		let balance = ports.ledger.balance(&key).await?;
		if balance.pending != 0 || balance.locked != 0 {
			return Err(DomainError::Precondition(format!(
				"'{}' has in-flight pending transfers — a legacy hand-over is still completing; wait for the outbox to drain and retry",
				key.logical_key()
			)));
		}
		let transfer_id = migration_id(&format!("company:{product}")).as_u128();
		let retired_earlier = ports.ledger.transfer_exists(transfer_id).await?;
		let units = Shares::from_base_units(balance.posted);
		if units.is_zero() {
			if retired_earlier {
				steps.push(CompanyStep {
					product,
					units,
					transfer_id,
					retired_earlier,
				});
			}
			continue;
		}
		match decision {
			CompanyStake::Keep => {
				return Err(DomainError::Precondition(format!(
					"'{product}' still carries a company stake of {} units (shares_company, code 63) and --company is 'keep' — the owners decide: rerun with --company retire to burn it, or move it by hand first",
					units.to_decimal_string()
				)));
			}
			CompanyStake::Retire => steps.push(CompanyStep {
				product,
				units,
				transfer_id,
				retired_earlier,
			}),
		}
	}
	Ok(steps)
}

fn overflow(service: &ServiceId) -> DomainError {
	DomainError::Repository(format!("'{service}' value overflows"))
}

/// Execute a plan: per allocation, `fund` then `fee`, the linked chain and then the
/// rows; then each company stake to retire. Every write is idempotent by its id, so a
/// plan whose steps are already applied writes nothing new and the report says so.
/// The read-back at the end is what the operator compares with the plan.
pub async fn run(ports: &MigrationPorts<'_>, plan: &MigrationPlan) -> Result<RunReport, DomainError> {
	let fund = run_allocation(ports, &plan.fund).await?;
	let fee = run_allocation(ports, &plan.fee).await?;
	let mut company_retired = Vec::new();
	for step in &plan.company {
		if step.retired_earlier || step.units.is_zero() {
			continue;
		}
		// The burn credits the retired stake account: the one place it is still written.
		#[allow(deprecated)]
		ports
			.ledger
			.post(&LedgerTransfer {
				id: step.transfer_id,
				debit: LedgerAccountKey::SharesOutstanding(step.product.clone()),
				credit: LedgerAccountKey::CompanyShares(step.product.clone()),
				amount: step.units.base_units(),
				code: TransferCode::UnitRetire,
				reference: namespace().as_u128(),
			})
			.await?;
		company_retired.push(step.product.clone());
	}
	let after = read_after(ports).await?;
	Ok(RunReport { fund, fee, company_retired, after })
}

async fn run_allocation(ports: &MigrationPorts<'_>, step: &AllocationStep) -> Result<StepOutcome, DomainError> {
	let mut outcome = StepOutcome::default();
	match step.status {
		StepStatus::Nothing => return Ok(outcome),
		StepStatus::Pending => {
			ports.ledger.post_linked(&step.legs(namespace().as_u128())).await?;
			outcome.chain_posted = true;
		}
		StepStatus::AlreadyApplied => {}
	}
	for grant in &step.grants {
		if grant.row_present {
			outcome.rows_already_present += 1;
			continue;
		}
		let issuance = UnitIssuance::applied_mint(
			grant.issuance_id,
			step.service.clone(),
			UnitHolder::User(grant.user),
			grant.units,
			step.nav,
			grant.idempotency_key.clone(),
		)?;
		if ports.issuances.record_applied(&issuance).await? {
			outcome.rows_written += 1;
		} else {
			outcome.rows_already_present += 1;
		}
	}
	Ok(outcome)
}

/// The ledger as the operator must see it after the run: the retired claims (read
/// through their deprecated keys — that they are empty is the point) and both reserved
/// allocations' ownership pictures.
#[allow(deprecated)]
pub async fn read_after(ports: &MigrationPorts<'_>) -> Result<AfterRun, DomainError> {
	let fund = ownership_app::allocation_ownership(ports.ledger, ServiceId::fund()).await?;
	let fee = ownership_app::allocation_ownership(ports.ledger, ServiceId::fee()).await?;
	Ok(AfterRun {
		retired_fund: Usdt::from_base_units(ports.ledger.balance(&LedgerAccountKey::Fund).await?.posted),
		retired_fee_revenue: Usdt::from_base_units(ports.ledger.balance(&LedgerAccountKey::FeeRevenue).await?.posted),
		fund_nav: funds_app::nav_of(ports.nav, ports.ledger, &ServiceId::fund()).await?.nav,
		fee_nav: funds_app::nav_of(ports.nav, ports.ledger, &ServiceId::fee()).await?.nav,
		fund,
		fee,
	})
}

impl fmt::Display for MigrationPlan {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		for step in [&self.fund, &self.fee] {
			writeln!(f, "== {} ==", step.service)?;
			writeln!(
				f,
				"  {:<28} {} USDT",
				format!("{} (retired, moves)", step.retired_key.logical_key()),
				step.retired.to_decimal_string()
			)?;
			writeln!(f, "  {:<28} {} USDT", format!("service:{} (before)", step.service), step.claim.to_decimal_string())?;
			for holding in &step.holdings {
				writeln!(
					f,
					"  {:<28} {} units × NAV {} = {} USDT",
					format!("fee class of {}", holding.product),
					holding.units.to_decimal_string(),
					holding.nav.to_decimal_string(),
					holding.value.to_decimal_string()
				)?;
			}
			writeln!(f, "  {:<28} {} USDT at NAV {}", "value", step.value.to_decimal_string(), step.nav.to_decimal_string())?;
			match step.status {
				StepStatus::Pending => writeln!(
					f,
					"  status: PENDING — one linked chain: {} cash leg + {} mints",
					usize::from(!step.retired.is_zero()),
					step.grants.len()
				)?,
				StepStatus::AlreadyApplied if step.rows_missing() == 0 => writeln!(f, "  status: already applied, every row present — nothing to do")?,
				StepStatus::AlreadyApplied => writeln!(
					f,
					"  status: already applied on the ledger; {} of {} rows missing, will be written from the ledger",
					step.rows_missing(),
					step.grants.len()
				)?,
				StepStatus::Nothing => writeln!(f, "  status: nothing to migrate (no value behind '{}')", step.service)?,
			}
			for grant in &step.grants {
				writeln!(
					f,
					"    {} {:>6} bps -> {} units{}{}",
					grant.user,
					grant.share_bps,
					grant.units.to_decimal_string(),
					if grant.posted { " [minted]" } else { "" },
					if grant.row_present { " [row]" } else { "" }
				)?;
			}
		}
		writeln!(f, "== company stakes (shares_company, code 63) ==")?;
		if self.company.is_empty() {
			writeln!(f, "  none outstanding")?;
		}
		for step in &self.company {
			if step.retired_earlier {
				writeln!(f, "  {:<28} retired by an earlier run", step.product)?;
			} else {
				writeln!(
					f,
					"  {:<28} {} units -> RETIRE (Dr shares_outstanding / Cr shares_company)",
					step.product,
					step.units.to_decimal_string()
				)?;
			}
		}
		Ok(())
	}
}

impl fmt::Display for AfterRun {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		writeln!(f, "  {:<28} {} USDT", "fund (retired, code 1)", self.retired_fund.to_decimal_string())?;
		writeln!(f, "  {:<28} {} USDT", "fee (retired, code 40)", self.retired_fee_revenue.to_decimal_string())?;
		for (picture, nav) in [(&self.fund, self.fund_nav), (&self.fee, self.fee_nav)] {
			writeln!(
				f,
				"  {:<28} claim {} USDT · units outstanding {} · held {} · NAV {}",
				format!("service:{}", picture.service),
				picture.claim.posted.to_decimal_string(),
				picture.units_outstanding.to_decimal_string(),
				picture.held_units().to_decimal_string(),
				nav.to_decimal_string()
			)?;
			for UnitHolding { holder, units } in &picture.holders {
				writeln!(f, "    {:<26} {} units", holder_label(holder), units.to_decimal_string())?;
			}
			for (product, units) in &picture.product_units {
				writeln!(f, "    {:<26} {} units", format!("fee class of {product}"), units.to_decimal_string())?;
			}
		}
		Ok(())
	}
}

fn holder_label(holder: &UnitHolder) -> String {
	match holder.user_id() {
		Some(user) => user.to_string(),
		None => format!("{}:{}", holder.kind_str(), holder.service_id().map(ToString::to_string).unwrap_or_default()),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn table(fund: &[(UserId, u32)], fee: &[(UserId, u32)]) -> HolderTable {
		let rows = |shares: &[(UserId, u32)]| {
			shares
				.iter()
				.map(|(user_id, share_bps)| HolderShare {
					user_id: *user_id,
					share_bps: *share_bps,
				})
				.collect()
		};
		HolderTable { fund: rows(fund), fee: rows(fee) }
	}

	#[test]
	fn a_holder_table_must_assign_the_whole_allocation_to_distinct_people() {
		let (a, b) = (UserId::new(), UserId::new());
		assert!(table(&[(a, 8000), (b, 2000)], &[(a, 10_000)]).validate().is_ok());
		assert!(table(&[(a, 8000), (b, 1000)], &[(a, 10_000)]).validate().is_err(), "9 000 bps is not the whole allocation");
		assert!(table(&[(a, 8000), (b, 3000)], &[(a, 10_000)]).validate().is_err(), "11 000 bps is more than the whole allocation");
		assert!(table(&[(a, 5000), (a, 5000)], &[(b, 10_000)]).validate().is_err(), "one person listed twice");
		assert!(table(&[(a, 10_000), (b, 0)], &[(b, 10_000)]).validate().is_err(), "a zero share is not a holder");
		assert!(table(&[], &[(b, 10_000)]).validate().is_err(), "an empty table has no holder");
	}

	#[test]
	fn the_table_parses_from_the_operators_json_and_refuses_unknown_keys() {
		let (a, b) = (UserId::new(), UserId::new());
		let raw = format!(r#"{{"fee": [{{"user_id": "{a}", "share_bps": 8000}}, {{"user_id": "{b}", "share_bps": 2000}}], "fund": [{{"user_id": "{a}", "share_bps": 10000}}]}}"#);
		let parsed = HolderTable::parse_json(&raw).unwrap();
		assert_eq!(parsed, table(&[(a, 10_000)], &[(a, 8000), (b, 2000)]));
		assert!(HolderTable::parse_json(r#"{"fee": [], "fund": [], "extra": 1}"#).is_err());
		assert!(HolderTable::parse_json("not json").is_err());
	}

	// The ids are the migration's memory: the same key must give the same id on every
	// machine and every run, and the mint id must be the one the relay derives.
	#[test]
	fn migration_ids_are_stable_and_the_mint_id_is_the_relays() {
		let user = UserId::from_raw(Uuid::nil());
		let key = grant_key(&ServiceId::fee(), user);
		assert_eq!(key.as_str(), "migrate-ownership:fee:00000000-0000-0000-0000-000000000000");
		let issuance = migration_id(&format!("{}:{user}", ServiceId::fee()));
		assert_eq!(issuance, migration_id("fee:00000000-0000-0000-0000-000000000000"));
		assert_ne!(issuance, migration_id("fund:00000000-0000-0000-0000-000000000000"));
		assert_eq!(issuance_mint_id(issuance), issuance_mint_id(issuance));
		assert_ne!(migration_id("fee:claim:x"), migration_id("fund:claim:x"));
	}

	#[test]
	fn company_decision_parses() {
		assert_eq!(CompanyStake::parse("keep").unwrap(), CompanyStake::Keep);
		assert_eq!(CompanyStake::parse("retire").unwrap(), CompanyStake::Retire);
		assert!(CompanyStake::parse("burn").is_err());
	}
}

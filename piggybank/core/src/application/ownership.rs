//! Who holds an allocation — the read behind the cap table, the treasury and the
//! reconciliation's per-allocation checks (#245).
//!
//! The invariant every PR of the ownership work is held to: **every unit of value on
//! every ledger is at a holder**, a person directly (a `user:<id>` claim) or through
//! the units of an allocation. This module is where that is *read*, never derived: an
//! allocation's holders are summed from the Share ledger's holding accounts, its supply
//! is read from `SharesOutstanding`, its cash from its claim, and the two figures a
//! reader compares — `Σ holders` and the supply — are both reported, so a remainder is
//! a finding rather than something a view fills in.
//!
//! Two things can be wrong with an allocation, and they are reported apart:
//! - **units drift** ([`AllocationOwnership::units_reconcile`] false) — the supply is
//!   not what the holders add up to. The ledger's double entry makes this impossible
//!   by construction, so a mismatch is a genuine inconsistency: an error.
//! - **value without a holder** ([`AllocationOwnership::is_unheld`]) — the allocation
//!   holds cash or product units while nobody holds units of it. Nothing is lost and
//!   nothing is inconsistent; the money is simply not yet anyone's. It is the expected
//!   state of the `fee` allocation between the first release and the ownership data
//!   migration, when fees settle into `service:fee` before its first holders are seated
//!   — so it is a warning and a number, not an alert — and it should be zero everywhere
//!   after that migration.

use std::collections::HashMap;

use domain::{
	balance::{LedgerAccountKey, ServiceId},
	error::DomainError,
	issuance::UnitHolder,
	money::{Shares, Usdt},
};

use crate::{
	application::issuance::UnitHolding,
	ports::ledger::{HoldingScope, Ledger, LedgerBalance},
};

/// An allocation's cash claim, split the way every reader of it needs: what is settled,
/// what a payment out of it has spoken for, and what is left to spend. Read off ONE
/// balance, so the three can never disagree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AllocationClaim {
	/// The settled balance of `service:<svc>`.
	pub posted: Usdt,
	/// `posted − reserved`.
	pub available: Usdt,
	/// Locked by approved payments out of the claim that have not settled.
	pub reserved: Usdt,
}

impl From<LedgerBalance> for AllocationClaim {
	fn from(balance: LedgerBalance) -> Self {
		Self {
			posted: Usdt::from_base_units(balance.posted),
			available: Usdt::from_base_units(balance.available()),
			reserved: Usdt::from_base_units(balance.locked),
		}
	}
}

/// One allocation's ownership picture, read live from the ledger.
#[derive(Clone, Debug)]
pub struct AllocationOwnership {
	pub service: ServiceId,
	/// The allocation's cash.
	pub claim: AllocationClaim,
	/// Units in circulation — what the cash and the held product units are divided over.
	pub units_outstanding: Shares,
	/// Who holds it, largest first; ties broken by the holder's stored identity, so the
	/// table reads the same on every refresh. Holders with nothing left are not listed.
	pub holders: Vec<UnitHolding>,
	/// Units of OTHER products this allocation holds — a reserved allocation's fee
	/// classes — the non-cash part of what stands behind its own units. Empty for a
	/// product: the holder graph is reserved → product, one hop.
	pub product_units: Vec<(ServiceId, Shares)>,
}

impl AllocationOwnership {
	/// `Σ holders` — what the supply must equal. Saturating: a sum past `u128` cannot
	/// equal any supply, so it reads as drift rather than wrapping into agreement.
	pub fn held_units(&self) -> Shares {
		self.holders
			.iter()
			.fold(Shares::ZERO, |sum, line| sum.checked_add(line.units).unwrap_or(Shares::from_base_units(u128::MAX)))
	}

	/// Whether the supply is what its holders add up to.
	pub fn units_reconcile(&self) -> bool {
		self.held_units() == self.units_outstanding
	}

	/// Whether anything stands behind the allocation's units — cash on its claim, or
	/// units of a product it holds.
	pub fn has_value(&self) -> bool {
		!self.claim.posted.is_zero() || self.product_units.iter().any(|(_, units)| !units.is_zero())
	}

	/// Value with nobody behind it: the allocation holds something while no unit of it
	/// is outstanding, so no person owns what it holds.
	pub fn is_unheld(&self) -> bool {
		self.units_outstanding.is_zero() && self.has_value()
	}
}

/// Read `service`'s ownership picture straight from the ledger (Read-First). Ungated on
/// the registry: the callers either walked the registry to get here or are checking the
/// ledger against itself.
///
/// Each account is read at its own instant, so the picture can be torn by a transfer
/// landing mid-read — a mint between the holders' scan and the supply read shows as a
/// one-unit drift. A view reports both figures and lets the reader compare; the
/// reconciliation takes a second look before it alerts.
pub async fn allocation_ownership(ledger: &dyn Ledger, service: ServiceId) -> Result<AllocationOwnership, DomainError> {
	let claim = AllocationClaim::from(ledger.balance(&LedgerAccountKey::ServiceClaim(service.clone())).await?);
	let units_outstanding = Shares::from_base_units(ledger.balance(&LedgerAccountKey::SharesOutstanding(service.clone())).await?.posted);
	let mut by_holder: HashMap<UnitHolder, Shares> = HashMap::new();
	for (key, units) in ledger.share_holdings(&HoldingScope::Product(service.clone())).await? {
		if units == 0 {
			continue;
		}
		let Some((_, holder)) = UnitHolder::of_holding(&key) else { continue };
		let line = by_holder.entry(holder).or_insert(Shares::ZERO);
		*line = line
			.checked_add(Shares::from_base_units(units))
			.ok_or_else(|| DomainError::Repository("a holder's units overflow".into()))?;
	}
	let mut holders: Vec<UnitHolding> = by_holder.into_iter().map(|(holder, units)| UnitHolding { holder, units }).collect();
	holders.sort_by(|a, b| b.units.cmp(&a.units).then_with(|| holder_identity(&a.holder).cmp(&holder_identity(&b.holder))));
	// Only a reserved allocation can hold another product's units (`UnitHolder::Allocation`
	// admits nothing else), so a product's scan would read the whole Share ledger to find
	// nothing.
	let mut product_units = Vec::new();
	if service.is_reserved() {
		for (key, units) in ledger.share_holdings(&HoldingScope::Holder(UnitHolder::Allocation(service.clone()))).await? {
			if units == 0 {
				continue;
			}
			let Some((product, _)) = UnitHolder::of_holding(&key) else { continue };
			product_units.push((product, Shares::from_base_units(units)));
		}
		product_units.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
	}
	Ok(AllocationOwnership {
		service,
		claim,
		units_outstanding,
		holders,
		product_units,
	})
}

/// The holder as its columns spell it — the tie-breaker of the cap table's order.
fn holder_identity(holder: &UnitHolder) -> (&'static str, String) {
	(
		holder.kind_str(),
		holder
			.user_id()
			.map(|u| u.to_string())
			.or_else(|| holder.service_id().map(ToString::to_string))
			.unwrap_or_default(),
	)
}

#[cfg(test)]
mod tests {
	use domain::users::UserId;

	use super::*;

	fn shares(decimal: &str) -> Shares {
		Shares::parse_decimal(decimal).unwrap()
	}

	fn usdt(decimal: &str) -> Usdt {
		Usdt::parse_decimal(decimal).unwrap()
	}

	fn picture(claim: &str, outstanding: &str, holders: &[&str], product_units: &[&str]) -> AllocationOwnership {
		AllocationOwnership {
			service: ServiceId::fee(),
			claim: AllocationClaim {
				posted: usdt(claim),
				available: usdt(claim),
				reserved: Usdt::ZERO,
			},
			units_outstanding: shares(outstanding),
			holders: holders
				.iter()
				.map(|units| UnitHolding {
					holder: UnitHolder::User(UserId::new()),
					units: shares(units),
				})
				.collect(),
			product_units: product_units.iter().map(|units| (ServiceId::parse("arb").unwrap(), shares(units))).collect(),
		}
	}

	#[test]
	fn the_supply_reconciles_when_the_holders_add_up_to_it() {
		assert!(picture("0", "30", &["10", "20"], &[]).units_reconcile());
		assert!(!picture("0", "30", &["10", "10"], &[]).units_reconcile(), "a unit nobody holds is drift");
		assert!(!picture("0", "10", &["10", "10"], &[]).units_reconcile(), "a unit held beyond the supply is drift");
		assert!(picture("0", "0", &[], &[]).units_reconcile(), "an empty allocation trivially reconciles");
	}

	#[test]
	fn value_is_unheld_only_while_nobody_holds_units_of_it() {
		// The window between the first release and the data migration: fees settled into
		// `service:fee`, no holder seated yet.
		assert!(picture("100", "0", &[], &[]).is_unheld());
		// The same for the fee classes it holds in a product.
		assert!(picture("0", "0", &[], &["5"]).is_unheld());
		// One holder seated: the same cash is theirs.
		assert!(!picture("100", "1", &["1"], &[]).is_unheld());
		// Nothing there, nobody there: nothing to report.
		assert!(!picture("0", "0", &[], &[]).is_unheld());
	}
}

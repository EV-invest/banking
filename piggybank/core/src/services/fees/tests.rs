//! The one piece of this surface the integration suites cannot reach without a full gRPC
//! harness: what an INVESTOR is shown of a change of terms versus what an OPERATOR is
//! (banking#250). The terms, the state, the moments and the reason are public; who asked
//! and which consilium decides are governance detail, blanked for everyone without
//! `AllocationManage`.

use domain::{
	balance::ServiceId,
	consilium::ConsiliumId,
	fees::{ChangeRequirement, CrystallizationPeriod, FeePolicy, FeePolicyChangeId, FeePolicyChangeState, ManagementBasis},
};
use uuid::Uuid;

use super::{Audience, change_to_proto, policy_to_proto};
use crate::{
	application::fees::PolicyView,
	ports::fees::{FeePolicyChange, PolicyRecord},
};

fn consilium_gated_change() -> FeePolicyChange {
	FeePolicyChange {
		id: FeePolicyChangeId::from_raw(Uuid::from_u128(0x233)),
		service: ServiceId::parse("trading").unwrap(),
		version: 2,
		policy: FeePolicy::new(300, 2_000, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual).unwrap(),
		state: FeePolicyChangeState::AwaitingConsilium,
		requirement: ChangeRequirement::OwnerConsilium,
		effective_from_unix: 1_700_100_000,
		consilium_id: Some(ConsiliumId::from_raw(Uuid::from_u128(0xc0))),
		requested_by: "8f0d2a8e-2b3e-4a1a-9a53-6d5a0d1d2e3f".to_owned(),
		requested_at_unix: 1_700_000_000,
		reason: "the new mandate costs more to run".to_owned(),
		scheduled_at_unix: None,
		applied_at_unix: None,
	}
}

#[test]
fn an_investor_is_shown_the_terms_but_not_who_asked_or_which_consilium_decides() {
	let change = consilium_gated_change();
	let shown = change_to_proto(&change, Audience::Investor);

	assert_eq!(shown.requested_by, "", "who asked is governance detail");
	assert_eq!(shown.consilium_id, "", "which consilium decides is governance detail");
	// Everything an investor is entitled to stays: the terms they will be on, when, and why.
	assert_eq!(shown.id, change.id.to_string());
	assert_eq!(shown.management_bps, 300);
	assert_eq!(shown.performance_bps, 2_000);
	assert_eq!(shown.state, "awaiting_consilium");
	assert_eq!(shown.requirement, "owner_consilium");
	assert_eq!(shown.effective_from, 1_700_100_000);
	assert_eq!(shown.requested_at, 1_700_000_000);
	assert_eq!(shown.reason, "the new mandate costs more to run");
}

#[test]
fn an_operator_is_shown_who_asked_and_which_consilium_decides() {
	let change = consilium_gated_change();
	let shown = change_to_proto(&change, Audience::Operator);

	assert_eq!(shown.requested_by, "8f0d2a8e-2b3e-4a1a-9a53-6d5a0d1d2e3f");
	assert_eq!(shown.consilium_id, ConsiliumId::from_raw(Uuid::from_u128(0xc0)).to_string());
}

#[test]
fn the_redaction_follows_the_pending_change_onto_the_policy_view() {
	// `GetFeePolicy` carries the change on its way inside the policy; the same audience rule
	// must apply there, or the blanking on `ListFeePolicyChanges` is one screen away from
	// being moot.
	let service = ServiceId::parse("trading").unwrap();
	let view = PolicyView {
		current: Some(PolicyRecord {
			policy: FeePolicy::HOUSE,
			version: 1,
			effective_from_unix: 1_690_000_000,
			updated_at_unix: 1_690_000_000,
		}),
		pending: Some(consilium_gated_change()),
	};

	let investor = policy_to_proto(&service, &view, Audience::Investor);
	let pending = investor.pending.expect("the change on its way is shown to everyone");
	assert_eq!(pending.requested_by, "");
	assert_eq!(pending.consilium_id, "");
	assert_eq!(investor.management_bps, 200, "the live terms are public");

	let operator = policy_to_proto(&service, &view, Audience::Operator);
	let pending = operator.pending.expect("the change on its way is shown to everyone");
	assert_ne!(pending.requested_by, "");
	assert_ne!(pending.consilium_id, "");
}

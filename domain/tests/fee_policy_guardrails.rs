//! Boundary tests for the fee-policy guardrails (#233), through the crate's public API.
//!
//! The unit tests beside the code pin the rules; these pin the EDGES the rules are stated
//! at — the exact second the notice period is satisfied, the exact byte a reason may still
//! have — and the one property the consilium's hash re-check silently depends on: that a
//! subject read back from JSONB, whose keys Postgres stores in its own order, hashes to
//! the bytes the owners signed. The reordered literal doubles as a pin on the stored key
//! names: every fee-policy consilium already in the database is written in them, and a
//! rename would make each of those rows unreadable.

use domain::{
	balance::ServiceId,
	fees::{CrystallizationPeriod, FeePolicy, FeePolicyChangeId, FeePolicySubject, MAX_REASON_BYTES, MIN_NOTICE_SECS, ManagementBasis, earliest_effective_from, validate_reason},
};

const SCHEDULED_AT: i64 = 1_700_000_000;

fn subject() -> FeePolicySubject {
	FeePolicySubject {
		change_id: FeePolicyChangeId::from_raw(uuid::Uuid::from_u128(0x233)),
		service: ServiceId::parse("trading").unwrap(),
		from: Some(FeePolicy::HOUSE),
		to: FeePolicy::new(300, 2_500, 800, ManagementBasis::MarketValue, CrystallizationPeriod::Quarterly).unwrap(),
		reason: "the new mandate costs more to run".to_owned(),
		requested_effective_from: SCHEDULED_AT + 3 * MIN_NOTICE_SECS,
	}
}

#[test]
fn a_subject_read_back_with_its_keys_in_another_order_hashes_to_the_same_bytes() {
	let signed = subject();
	// The wire form, so the identifiers are spelled exactly as serde spells them.
	let as_written: serde_json::Value = serde_json::from_str(&serde_json::to_string(&signed).unwrap()).unwrap();
	let change_id = as_written["change_id"].as_str().expect("the change id is a string on the wire");
	let service = as_written["service"].as_str().expect("the service is a string on the wire");

	// Every key in the reverse of the struct's order, at both levels — the shape JSONB
	// (which stores keys by length, then bytewise) or any other store may hand back.
	let reordered = format!(
		r#"{{
			"requested_effective_from": {requested},
			"reason": "the new mandate costs more to run",
			"to": {{"crystallization": "quarterly", "basis": "market_value", "hurdle_bps": 800, "performance_bps": 2500, "management_bps": 300}},
			"from": {{"crystallization": "annual", "basis": "invested_capital", "hurdle_bps": 0, "performance_bps": 2000, "management_bps": 200}},
			"service": "{service}",
			"change_id": "{change_id}"
		}}"#,
		requested = SCHEDULED_AT + 3 * MIN_NOTICE_SECS,
	);
	let read_back: FeePolicySubject = serde_json::from_str(&reordered).unwrap();

	assert_eq!(read_back, signed);
	assert_eq!(read_back.canonical_bytes(), signed.canonical_bytes(), "the hash the owners signed must survive the store");
}

#[test]
fn a_subject_whose_terms_were_edited_in_the_store_hashes_differently() {
	let signed = subject();
	let mut tampered: serde_json::Value = serde_json::from_str(&serde_json::to_string(&signed).unwrap()).unwrap();
	tampered["to"]["management_bps"] = serde_json::json!(301);
	let read_back: FeePolicySubject = serde_json::from_value(tampered).unwrap();

	assert_ne!(read_back.canonical_bytes(), signed.canonical_bytes(), "one basis point more is not what the owners signed");
}

#[test]
fn the_notice_period_is_satisfied_at_exactly_twenty_four_hours_and_not_a_second_earlier() {
	let one_second_short = SCHEDULED_AT + MIN_NOTICE_SECS - 1;
	assert_eq!(
		earliest_effective_from(SCHEDULED_AT, one_second_short, true),
		SCHEDULED_AT + MIN_NOTICE_SECS,
		"a second short of the notice is lifted to it"
	);
	let exactly = SCHEDULED_AT + MIN_NOTICE_SECS;
	assert_eq!(earliest_effective_from(SCHEDULED_AT, exactly, true), exactly, "exactly the notice period is honoured as asked");
	let one_second_over = SCHEDULED_AT + MIN_NOTICE_SECS + 1;
	assert_eq!(earliest_effective_from(SCHEDULED_AT, one_second_over, true), one_second_over);
}

#[test]
fn a_request_dated_before_the_scheduling_moment_binds_no_earlier_than_that_moment() {
	// "As soon as allowed" spelled as a moment in the past, or as a negative number, is
	// lifted to the scheduling moment — never honoured as a retroactive change.
	assert_eq!(earliest_effective_from(SCHEDULED_AT, SCHEDULED_AT - 1, false), SCHEDULED_AT);
	assert_eq!(earliest_effective_from(SCHEDULED_AT, -1, false), SCHEDULED_AT);
	assert_eq!(earliest_effective_from(SCHEDULED_AT, i64::MIN, true), SCHEDULED_AT + MIN_NOTICE_SECS);
}

#[test]
fn a_reason_may_fill_its_whole_budget_but_not_one_byte_more() {
	assert!(validate_reason(&"x".repeat(MAX_REASON_BYTES), true).is_ok());
	assert!(validate_reason(&"x".repeat(MAX_REASON_BYTES + 1), true).is_err());
	// The budget is in BYTES, not characters: a multi-byte reason is held to the same wire size.
	let cyrillic = "ж".repeat(MAX_REASON_BYTES / 2);
	assert_eq!(cyrillic.len(), MAX_REASON_BYTES);
	assert!(validate_reason(&cyrillic, true).is_ok());
	assert!(validate_reason(&format!("{cyrillic}a"), true).is_err());
}

//! The `spend_brake` row — the operator's emergency brake on the signer, read before every
//! signature (migration 0009).
//!
//! The environment's ceilings ([`crate::policy`]) are loaded once at boot; this row is what an
//! operator can change while the process runs, with one `UPDATE` at the signer's own database.
//! It can only tighten: [`crate::policy::SignerPolicy::tightened`] takes `min(env, brake)` per
//! ceiling, so a value above the environment's leaves the environment in force, and `halted`
//! refuses every signature outright. There is no RPC to it — the hub, which holds a service
//! token for every signer RPC, is the adversary the brake exists to stop.
//!
//! Fail-closed all the way: a read error is a refusal, and so is a MISSING row. The signer
//! cannot tell a deleted row from a broken database, and "the operator deleted the brake" must
//! never come out as "no brake". Both surface as [`SignerError::Repository`] → `Internal`,
//! which the hub answers by parking the withdrawal, the same as a policy verdict.
//!
//! A change to the row is an event worth a log line of its own — above all a RELEASE, which
//! is otherwise visible only as refusals stopping. [`BrakeWatch`] keeps the last snapshot the
//! service saw and reports the first one and every change, so `service` logs the transition
//! in either direction from the signer's side; the database keeps its own trace in
//! `spend_brake_history` (0009), and the two are independent witnesses.
//!
//! The NUMERIC columns cross the wire as decimal text and the timestamp as Postgres' own
//! rendering, like every other store here: a `u128` fits no sqlx-native type, and the
//! timestamp is only ever logged.

use std::sync::{Mutex, PoisonError};

use domain::money::Network;
use sqlx::PgPool;
use tonic::Status;

use crate::error::SignerError;

/// The brake's per-rail ceilings on the native spend window, in base units; `None` leaves the
/// environment's ceiling for that rail.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NativeSpendBrake {
	bep20_wei: Option<u128>,
	polygon_wei: Option<u128>,
	trc20_sun: Option<u128>,
	ton_nano: Option<u128>,
}

impl NativeSpendBrake {
	pub fn cap(&self, network: Network) -> Option<u128> {
		match network {
			Network::Bep20 => self.bep20_wei,
			Network::Polygon => self.polygon_wei,
			Network::Trc20 => self.trc20_sun,
			Network::Ton => self.ton_nano,
		}
	}

	fn set(&mut self, network: Network, cap: Option<u128>) {
		match network {
			Network::Bep20 => self.bep20_wei = cap,
			Network::Polygon => self.polygon_wei = cap,
			Network::Trc20 => self.trc20_sun = cap,
			Network::Ton => self.ton_nano = cap,
		}
	}

	fn engages(&self) -> bool {
		self.bep20_wei.is_some() || self.polygon_wei.is_some() || self.trc20_sun.is_some() || self.ton_nano.is_some()
	}
}

/// One snapshot of the brake row, consistent within the request that read it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpendBrake {
	halted: bool,
	max_transfer_usdt: Option<u64>,
	max_treasury_usdt_per_hour: Option<u64>,
	native_spend: NativeSpendBrake,
	reason: Option<String>,
	/// Postgres' own rendering of `updated_at` — carried for the log lines only.
	updated_at: String,
}

impl SpendBrake {
	/// The seeded posture: released, every ceiling left to the environment. The constructor
	/// the pure tests build from; production only ever reads the row.
	pub fn released() -> Self {
		Self {
			halted: false,
			max_transfer_usdt: None,
			max_treasury_usdt_per_hour: None,
			native_spend: NativeSpendBrake::default(),
			reason: None,
			updated_at: String::new(),
		}
	}

	pub fn with_max_transfer_usdt(mut self, usdt: u64) -> Self {
		self.max_transfer_usdt = Some(usdt);
		self
	}

	pub fn with_max_treasury_usdt_per_hour(mut self, usdt: u64) -> Self {
		self.max_treasury_usdt_per_hour = Some(usdt);
		self
	}

	pub fn with_native_spend(mut self, network: Network, cap: u128) -> Self {
		self.native_spend.set(network, Some(cap));
		self
	}

	pub fn halted_with(mut self, reason: Option<&str>) -> Self {
		self.halted = true;
		self.reason = reason.map(str::to_owned);
		self
	}

	pub fn halted(&self) -> bool {
		self.halted
	}

	pub fn reason(&self) -> Option<&str> {
		self.reason.as_deref()
	}

	pub fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub fn max_transfer_usdt(&self) -> Option<u64> {
		self.max_transfer_usdt
	}

	pub fn max_treasury_usdt_per_hour(&self) -> Option<u64> {
		self.max_treasury_usdt_per_hour
	}

	pub fn native_spend(&self) -> &NativeSpendBrake {
		&self.native_spend
	}

	/// True when any ceiling column is set — the brake changes at least one number for this
	/// request, which is worth a log line; a halt is reported separately.
	pub fn engages(&self) -> bool {
		self.max_transfer_usdt.is_some() || self.max_treasury_usdt_per_hour.is_some() || self.native_spend.engages()
	}

	/// The halt verdict: `permission_denied` naming the brake and the operator's reason, so the
	/// parked withdrawal's status in `/admin/outbox` says why. `Ok` when released.
	pub fn require_released(&self) -> Result<(), Status> {
		if !self.halted {
			return Ok(());
		}
		let reason = match &self.reason {
			Some(reason) => format!(" ({reason})"),
			None => String::new(),
		};
		Err(Status::permission_denied(format!(
			"the signer's spend brake is halted{reason}: no signature is issued until an operator releases it at the signer's database"
		)))
	}
}

/// The brake row over the signer's own database.
#[derive(Clone)]
pub struct SpendBrakeStore {
	pool: PgPool,
}

/// The row as SELECTed: NUMERIC as text, the timestamp as text (see the module doc).
type BrakeRow = (
	bool,
	Option<i64>,
	Option<i64>,
	Option<String>,
	Option<String>,
	Option<String>,
	Option<String>,
	Option<String>,
	String,
);

impl SpendBrakeStore {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}

	/// One SELECT of the row. Fail-closed: any error, and a missing row, is
	/// [`SignerError::Repository`] — never a released brake.
	pub async fn read(&self) -> Result<SpendBrake, SignerError> {
		let row: Option<BrakeRow> = sqlx::query_as(
			"SELECT halted, max_transfer_usdt, max_treasury_usdt_per_hour, \
			 max_native_spend_per_hour_bep20::text, max_native_spend_per_hour_polygon::text, \
			 max_native_spend_per_hour_trc20::text, max_native_spend_per_hour_ton::text, \
			 reason, updated_at::text FROM spend_brake WHERE id = 1",
		)
		.fetch_optional(&self.pool)
		.await?;
		let Some((halted, max_transfer_usdt, max_treasury_usdt_per_hour, bep20, polygon, trc20, ton, reason, updated_at)) = row else {
			return Err(SignerError::Repository("spend_brake row is missing — refusing to sign".to_owned()));
		};
		let mut native_spend = NativeSpendBrake::default();
		for (network, raw) in [(Network::Bep20, bep20), (Network::Polygon, polygon), (Network::Trc20, trc20), (Network::Ton, ton)] {
			native_spend.set(network, raw.map(|raw| parse_native_cap(network, &raw)).transpose()?);
		}
		Ok(SpendBrake {
			halted,
			max_transfer_usdt: max_transfer_usdt.map(|raw| parse_usdt_cap("max_transfer_usdt", raw)).transpose()?,
			max_treasury_usdt_per_hour: max_treasury_usdt_per_hour.map(|raw| parse_usdt_cap("max_treasury_usdt_per_hour", raw)).transpose()?,
			native_spend,
			reason,
			updated_at,
		})
	}
}

/// The CHECK constraints keep these positive; a value that still does not fit is a broken
/// row, and a broken row refuses.
fn parse_usdt_cap(column: &str, raw: i64) -> Result<u64, SignerError> {
	u64::try_from(raw).map_err(|_| SignerError::Repository(format!("spend_brake.{column} is not a positive integer: {raw}")))
}

fn parse_native_cap(network: Network, raw: &str) -> Result<u128, SignerError> {
	raw.parse::<u128>()
		.map_err(|_| SignerError::Repository(format!("spend_brake native ceiling for {network} is not a u128: {raw:?}")))
}

/// What one [`BrakeWatch::observe`] saw: the first snapshot since boot, the same row as last
/// time, or a different one (the previous snapshot travels with it for the log line).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Observation {
	First,
	Unchanged,
	// Boxed: the enum is otherwise 200+ bytes for two dataless variants (clippy's
	// `large_enum_variant`), and `previous` only ever lives until it is logged.
	Changed { previous: Box<SpendBrake> },
}

/// The last brake snapshot the service saw, so a change is noticed exactly once — by the
/// first request that reads the new row. Whole-row equality on purpose: `updated_at` is part
/// of it, so an UPDATE that assigns the same values is a change too (the operator's hand was
/// on the brake, and the history table records it either way).
#[derive(Debug, Default)]
pub struct BrakeWatch {
	last: Mutex<Option<SpendBrake>>,
}

impl BrakeWatch {
	pub fn new() -> Self {
		Self::default()
	}

	/// Compare `now` with the last snapshot and remember `now`.
	pub fn observe(&self, now: &SpendBrake) -> Observation {
		// The guarded value is a plain snapshot that a panic between lock and unlock cannot
		// leave half-written, so a poisoned lock is as good as a clean one.
		let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
		let observation = match last.take() {
			None => Observation::First,
			Some(previous) if previous == *now => Observation::Unchanged,
			Some(previous) => Observation::Changed { previous: Box::new(previous) },
		};
		*last = Some(now.clone());
		observation
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn released_at(updated_at: &str) -> SpendBrake {
		SpendBrake {
			updated_at: updated_at.to_owned(),
			..SpendBrake::released()
		}
	}

	#[test]
	fn the_first_snapshot_is_first_and_the_same_snapshot_is_unchanged() {
		let watch = BrakeWatch::new();
		let released = released_at("2026-09-17 10:00:00+00");
		assert_eq!(watch.observe(&released), Observation::First);
		assert_eq!(watch.observe(&released), Observation::Unchanged);
		assert_eq!(watch.observe(&released.clone()), Observation::Unchanged);
	}

	#[test]
	fn a_halt_and_a_release_are_each_reported_once_with_the_previous_snapshot() {
		let watch = BrakeWatch::new();
		let released = released_at("2026-09-17 10:00:00+00");
		let halted = SpendBrake {
			updated_at: "2026-09-17 10:05:00+00".to_owned(),
			..SpendBrake::released().halted_with(Some("drill"))
		};
		let released_again = released_at("2026-09-17 10:10:00+00");

		assert_eq!(watch.observe(&released), Observation::First);
		assert_eq!(
			watch.observe(&halted),
			Observation::Changed {
				previous: Box::new(released.clone())
			}
		);
		assert_eq!(watch.observe(&halted), Observation::Unchanged);
		assert_eq!(watch.observe(&released_again), Observation::Changed { previous: Box::new(halted) });
		assert_eq!(watch.observe(&released_again), Observation::Unchanged);
	}

	#[test]
	fn a_ceiling_set_or_cleared_is_a_change() {
		let watch = BrakeWatch::new();
		let plain = released_at("t1");
		let capped = SpendBrake {
			updated_at: "t2".to_owned(),
			..SpendBrake::released().with_max_transfer_usdt(50)
		};
		assert_eq!(watch.observe(&plain), Observation::First);
		assert_eq!(watch.observe(&capped), Observation::Changed { previous: Box::new(plain.clone()) });
		assert_eq!(watch.observe(&plain), Observation::Changed { previous: Box::new(capped) });
	}

	#[test]
	fn an_update_that_only_moves_updated_at_is_a_change() {
		let watch = BrakeWatch::new();
		let first = released_at("t1");
		let touched = released_at("t2");
		assert_eq!(watch.observe(&first), Observation::First);
		assert_eq!(watch.observe(&touched), Observation::Changed { previous: Box::new(first) });
	}
}

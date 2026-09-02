//! Browser-facing JSON DTOs. These reproduce the exact wire shape the old TS BFF
//! emitted (proto-loader with `keepCase` + `longs: String`): snake_case fields, with
//! 64-bit integers rendered as strings so the committed `shared/contracts/gen` types
//! stay valid and the React fetch code is unchanged.

use evbanking_contracts::banking::v1 as bk;
use evconcierge_contracts::concierge::v1 as cc;
use serde::Serialize;

/// Declares one of the list DTOs: a `Vec` of an already-converted element type, the scalar
/// fields the wire shape carries alongside it, and the `From` that builds it from the proto
/// message. Every name is spelled out at the call site rather than derived from the DTO's,
/// because none of them line up reliably — the field is not always the plural of the element
/// type (`ParkedEventList.events`), and the source message is not always `<Dto>`
/// (`AdminUserList` comes from `cc::ListUsersResponse`).
///
/// The second form names the source binding so extra fields can be given as expressions over
/// it, which is what a field needing more than a move requires (`total: r.total.to_string()`).
macro_rules! list_dto {
	($(#[$meta:meta])* $name:ident from $src:path { $field:ident: Vec<$elem:ty> $(,)? }) => {
		list_dto! { $(#[$meta])* $name from $src as l { $field: Vec<$elem> } }
	};
	(
		$(#[$meta:meta])*
		$name:ident from $src:path as $msg:ident {
			$field:ident: Vec<$elem:ty>
			$(, $(#[$xmeta:meta])* $extra:ident: $xty:ty = $value:expr)* $(,)?
		}
	) => {
		$(#[$meta])*
		#[derive(Serialize)]
		pub struct $name {
			pub $field: Vec<$elem>,
			$($(#[$xmeta])* pub $extra: $xty,)*
		}

		impl From<$src> for $name {
			fn from($msg: $src) -> Self {
				Self {
					$field: $msg.$field.into_iter().map(<$elem>::from).collect(),
					$($extra: $value,)*
				}
			}
		}
	};
}

// ── /api/auth/session ────────────────────────────────────────────────────────
// Note the camelCase principal: the old BFF mapped the proto `user_id` → `userId`
// for this endpoint only (the session principal), unlike the snake_case passthroughs.

// ── concierge: user profile + sessions ───────────────────────────────────────

#[derive(Serialize)]
pub struct UserProfile {
	pub user_id: String,
	pub email: String,
	pub email_verified: bool,
	pub status: String,
	pub token_version: String,
	pub legal_name: String,
	pub preferred_name: String,
	pub phone: String,
	pub date_of_birth: String,
	pub nationality: String,
	pub tax_residence: String,
	pub residential_address: String,
	pub language: String,
	pub base_currency: String,
	pub timezone: String,
	pub kyc_level: u32,
	pub role: String,
}

impl From<cc::UserProfile> for UserProfile {
	fn from(p: cc::UserProfile) -> Self {
		Self {
			user_id: p.user_id,
			email: p.email,
			email_verified: p.email_verified,
			status: p.status,
			token_version: p.token_version.to_string(),
			legal_name: p.legal_name,
			preferred_name: p.preferred_name,
			phone: p.phone,
			date_of_birth: p.date_of_birth,
			nationality: p.nationality,
			tax_residence: p.tax_residence,
			residential_address: p.residential_address,
			language: p.language,
			base_currency: p.base_currency,
			timezone: p.timezone,
			kyc_level: p.kyc_level,
			role: p.role,
		}
	}
}

// ── piggybank: wallet ────────────────────────────────────────────────────────

#[derive(Default, Serialize)]
pub struct Balance {
	pub available: String,
	pub invested: String,
	pub pending_withdrawal: String,
	pub total: String,
}

impl From<bk::Balance> for Balance {
	fn from(b: bk::Balance) -> Self {
		Self {
			available: b.available,
			invested: b.invested,
			pending_withdrawal: b.pending_withdrawal,
			total: b.total,
		}
	}
}

#[derive(Serialize)]
pub struct DepositAddress {
	pub network: String,
	pub address: String,
	pub min_confirmations: u32,
	pub is_testnet: bool,
}

impl From<bk::DepositAddress> for DepositAddress {
	fn from(d: bk::DepositAddress) -> Self {
		Self {
			network: d.network,
			address: d.address,
			min_confirmations: d.min_confirmations,
			is_testnet: d.is_testnet,
		}
	}
}

#[derive(Serialize)]
pub struct NetworkWithdrawable {
	pub network: String,
	pub withdrawable: String,
	pub instant: String,
	pub min_withdrawal: String,
	pub withdrawal_fee: String,
}

impl From<bk::NetworkWithdrawable> for NetworkWithdrawable {
	fn from(n: bk::NetworkWithdrawable) -> Self {
		Self {
			network: n.network,
			withdrawable: n.withdrawable,
			instant: n.instant,
			min_withdrawal: n.min_withdrawal,
			withdrawal_fee: n.withdrawal_fee,
		}
	}
}

#[derive(Serialize)]
pub struct Wallet {
	pub balance: Balance,
	pub deposit_addresses: Vec<DepositAddress>,
	pub withdrawable: Vec<NetworkWithdrawable>,
}

impl From<bk::Wallet> for Wallet {
	fn from(w: bk::Wallet) -> Self {
		Self {
			balance: w.balance.map(Balance::from).unwrap_or_default(),
			deposit_addresses: w.deposit_addresses.into_iter().map(DepositAddress::from).collect(),
			withdrawable: w.withdrawable.into_iter().map(NetworkWithdrawable::from).collect(),
		}
	}
}

#[derive(Serialize)]
pub struct Withdrawal {
	pub id: String,
	pub network: String,
	pub address: String,
	pub amount: String,
	pub fee: String,
	pub net_amount: String,
	pub state: String,
	pub tx_ref: String,
}

impl From<bk::Withdrawal> for Withdrawal {
	fn from(w: bk::Withdrawal) -> Self {
		Self {
			id: w.id,
			network: w.network,
			address: w.address,
			amount: w.amount,
			fee: w.fee,
			net_amount: w.net_amount,
			state: w.state,
			tx_ref: w.tx_ref,
		}
	}
}

list_dto! { WithdrawalList from bk::WithdrawalList { withdrawals: Vec<Withdrawal> } }

#[derive(Serialize)]
pub struct Deposit {
	pub tx_ref: String,
	pub network: String,
	pub amount: String,
	pub created_at: String,
}

impl From<bk::Deposit> for Deposit {
	fn from(d: bk::Deposit) -> Self {
		Self {
			tx_ref: d.tx_ref,
			network: d.network,
			amount: d.amount,
			created_at: d.created_at.to_string(),
		}
	}
}

list_dto! { DepositList from bk::DepositList { deposits: Vec<Deposit> } }

// ── piggybank: operations (the unified activity timeline) ────────────────────

/// One row of the caller's activity timeline. `kind` is the discriminator: it decides
/// which of the remaining fields carry a value, and the rest arrive as empty strings
/// (proto3 defaults). Passed through verbatim — the BFF adds no interpretation to a
/// read model that already has exactly one shape.
#[derive(Serialize)]
pub struct Operation {
	pub id: String,
	pub kind: String,
	pub state: String,
	/// Unix seconds, rendered as a string like every other timestamp on this surface —
	/// an i64 does not survive JSON's 2^53 in every client.
	pub created_at: String,
	pub amount: String,
	pub fee: String,
	pub net_amount: String,
	pub units: String,
	pub nav: String,
	pub service: String,
	pub network: String,
	pub address: String,
	pub tx_ref: String,
	/// fee only — the two legs of a charge, so a timeline row can say whether it was rent
	/// on the capital or a share of the gain rather than only how much was taken.
	pub management: String,
	pub performance: String,
}

impl From<bk::Operation> for Operation {
	fn from(o: bk::Operation) -> Self {
		Self {
			id: o.id,
			kind: o.kind,
			state: o.state,
			created_at: o.created_at.to_string(),
			amount: o.amount,
			fee: o.fee,
			net_amount: o.net_amount,
			units: o.units,
			nav: o.nav,
			service: o.service,
			network: o.network,
			address: o.address,
			tx_ref: o.tx_ref,
			management: o.management,
			performance: o.performance,
		}
	}
}

list_dto! {
	OperationList from bk::OperationList as l {
		operations: Vec<Operation>,
		/// True when the history is longer than the page returned.
		truncated: bool = l.truncated,
	}
}

// ── piggybank: fees (operator) ───────────────────────────────────────────────

list_dto! {
	/// Every configured policy, for the operator's fees table. The element type is the same
	/// `FeePolicy` the investor read returns — the operator sees no more of a fund's terms
	/// than the people paying them do.
	FeePolicyList from bk::FeePolicyList { policies: Vec<FeePolicy> }
}

/// The manager's uncollected fee units in one fund. `value` is `units × current NAV` —
/// what a settlement would convert them to if it ran now.
#[derive(Serialize)]
pub struct FeeShares {
	pub service: String,
	pub units: String,
	pub value: String,
}

impl From<bk::FeeShares> for FeeShares {
	fn from(f: bk::FeeShares) -> Self {
		Self {
			service: f.service,
			units: f.units,
			value: f.value,
		}
	}
}

#[derive(Serialize)]
pub struct FeeSettlement {
	pub service: String,
	pub units: String,
	pub nav: String,
	pub cash: String,
}

impl From<bk::FeeSettlement> for FeeSettlement {
	fn from(s: bk::FeeSettlement) -> Self {
		Self {
			service: s.service,
			units: s.units,
			nav: s.nav,
			cash: s.cash,
		}
	}
}

/// One charge against one holding. `charged_cash` is below `management + performance`
/// exactly when the holding could not cover the whole charge and the rest went to
/// `debt_carried`.
#[derive(Serialize)]
pub struct FeeAssessment {
	pub service: String,
	pub trigger: String,
	pub nav: String,
	pub management: String,
	pub performance: String,
	pub debt_opening: String,
	pub charged_units: String,
	pub charged_cash: String,
	pub debt_carried: String,
	pub high_water_mark: String,
	pub assessed_at: String,
}

impl From<bk::FeeAssessment> for FeeAssessment {
	fn from(a: bk::FeeAssessment) -> Self {
		Self {
			service: a.service,
			trigger: a.trigger,
			nav: a.nav,
			management: a.management,
			performance: a.performance,
			debt_opening: a.debt_opening,
			charged_units: a.charged_units,
			charged_cash: a.charged_cash,
			debt_carried: a.debt_carried,
			high_water_mark: a.high_water_mark,
			assessed_at: a.assessed_at.to_string(),
		}
	}
}

list_dto! { FeeAssessmentList from bk::FeeAssessmentList { assessments: Vec<FeeAssessment> } }

// ── piggybank: allocations (the registry of investable products) ─────────────

/// One catalog entry. Carries no money — units/NAV/P&L come from the funds surface
/// keyed by the same `service`.
#[derive(Serialize)]
pub struct Allocation {
	pub service: String,
	pub title: String,
	pub summary: String,
	/// `draft` | `open` | `closed`.
	pub state: String,
	pub created_at: String,
	pub updated_at: String,
	/// Authorised unit supply, decimal. Subscribe refuses a mint that would pass it.
	pub unit_cap: String,
}

impl From<bk::Allocation> for Allocation {
	fn from(a: bk::Allocation) -> Self {
		Self {
			service: a.service,
			title: a.title,
			summary: a.summary,
			state: a.state,
			unit_cap: a.unit_cap,
			created_at: a.created_at.to_string(),
			updated_at: a.updated_at.to_string(),
		}
	}
}

list_dto! { AllocationList from bk::AllocationList { allocations: Vec<Allocation> } }

// ── piggybank: funds (the service currency) ──────────────────────────────────

#[derive(Serialize)]
pub struct Position {
	pub service: String,
	pub units: String,
	pub nav: String,
	pub value: String,
	pub cost_basis: String,
	pub pnl: String,
	pub nav_as_of: String,
}

impl From<bk::Position> for Position {
	fn from(p: bk::Position) -> Self {
		Self {
			service: p.service,
			units: p.units,
			nav: p.nav,
			value: p.value,
			cost_basis: p.cost_basis,
			pnl: p.pnl,
			nav_as_of: p.nav_as_of.to_string(),
		}
	}
}

list_dto! { PositionList from bk::PositionList { positions: Vec<Position> } }

#[derive(Serialize)]
pub struct Subscription {
	pub id: String,
	pub service: String,
	pub cash: String,
	pub nav: String,
	pub units: String,
}

impl From<bk::Subscription> for Subscription {
	fn from(s: bk::Subscription) -> Self {
		Self {
			id: s.id,
			service: s.service,
			cash: s.cash,
			nav: s.nav,
			units: s.units,
		}
	}
}

#[derive(Serialize)]
pub struct Redemption {
	pub id: String,
	pub service: String,
	pub units: String,
	pub nav: String,
	pub cash: String,
	pub state: String,
}

impl From<bk::Redemption> for Redemption {
	fn from(r: bk::Redemption) -> Self {
		Self {
			id: r.id,
			service: r.service,
			units: r.units,
			nav: r.nav,
			cash: r.cash,
			state: r.state,
		}
	}
}

list_dto! { RedemptionList from bk::RedemptionList { redemptions: Vec<Redemption> } }

#[derive(Serialize)]
pub struct FundNav {
	pub service: String,
	pub nav: String,
	pub aum: String,
	pub units_outstanding: String,
	pub posted_at: String,
	pub stale: bool,
	pub unit_cap: String,
	/// Units still issuable — already nets off in-flight mints, so a screen offering it
	/// can never offer more than Subscribe accepts.
	pub remaining_capacity: String,
}

impl From<bk::FundNav> for FundNav {
	fn from(f: bk::FundNav) -> Self {
		Self {
			service: f.service,
			nav: f.nav,
			aum: f.aum,
			units_outstanding: f.units_outstanding,
			posted_at: f.posted_at.to_string(),
			stale: f.stale,
			unit_cap: f.unit_cap,
			remaining_capacity: f.remaining_capacity,
		}
	}
}

// ── fees ──────────────────────────────────────────────────────────────────────

/// A fund's fee terms as the investor should read them. `configured` distinguishes "this
/// product charges nothing" from "we could not tell you" — the client renders no fee
/// section at all in the first case rather than a row of zeros that looks like a policy.
#[derive(Serialize)]
pub struct FeePolicy {
	pub service: String,
	pub configured: bool,
	pub management_bps: u32,
	pub performance_bps: u32,
	pub hurdle_bps: u32,
	pub basis: String,
	pub crystallization: String,
	pub updated_at: String,
}

impl From<bk::FeePolicy> for FeePolicy {
	fn from(p: bk::FeePolicy) -> Self {
		Self {
			service: p.service,
			configured: p.configured,
			management_bps: p.management_bps,
			performance_bps: p.performance_bps,
			hurdle_bps: p.hurdle_bps,
			basis: p.basis,
			crystallization: p.crystallization,
			updated_at: p.updated_at.to_string(),
		}
	}
}

/// What a holding owes right now without being charged. `high_water_mark` travels with it
/// because it is the single number that explains why two investors in the same fund at the
/// same price owe different performance fees.
#[derive(Serialize)]
pub struct AccruedFees {
	pub service: String,
	pub configured: bool,
	pub management: String,
	pub performance: String,
	pub debt: String,
	pub total: String,
	pub high_water_mark: String,
}

impl From<bk::AccruedFees> for AccruedFees {
	fn from(a: bk::AccruedFees) -> Self {
		Self {
			service: a.service,
			configured: a.configured,
			management: a.management,
			performance: a.performance,
			debt: a.debt,
			total: a.total,
			high_water_mark: a.high_water_mark,
		}
	}
}

// ── admin console ─────────────────────────────────────────────────────────────

/// One fleet-health row (Overview). Backend-sourced where a plane serves it; the
/// frontend renders the rest (Sentry/PostHog/incidents) against the shared obs libs.
#[derive(Serialize)]
pub struct FleetService {
	pub name: String,
	pub kind: String,
	pub status: String,
	pub detail: String,
}

/// Per-rail deposit scan-cursor age from Readiness — a growing age means deposits are
/// confirming on-chain but not being credited.
#[derive(Serialize)]
pub struct DepositScan {
	pub network: String,
	pub age_secs: String,
}

#[derive(Serialize)]
pub struct AdminOverview {
	pub services: Vec<FleetService>,
	/// Parked outbox rows on the money plane (the "money didn't move" set), from Readiness.
	pub parked_rows: String,
	pub backlog: String,
	pub oldest_backlog_age_secs: String,
	pub deposit_scan: Vec<DepositScan>,
	/// Signer unseal failures on money-moving paths since the hub booted — any non-zero
	/// value means a provably dead key (KEK epoch) was asked to sign; funds are stranded.
	pub unseal_failures: String,
}

/// A user row in the operator user list.
#[derive(Serialize)]
pub struct AdminUserSummary {
	pub user_id: String,
	pub email: String,
	pub status: String,
	pub kyc_level: u32,
	pub role: String,
	pub token_version: String,
	pub created_at: String,
}

impl From<cc::AdminUserSummary> for AdminUserSummary {
	fn from(u: cc::AdminUserSummary) -> Self {
		Self {
			user_id: u.user_id,
			email: u.email,
			status: u.status,
			kyc_level: u.kyc_level,
			role: u.role,
			token_version: u.token_version.to_string(),
			created_at: u.created_at.to_string(),
		}
	}
}

list_dto! {
	AdminUserList from cc::ListUsersResponse as r {
		users: Vec<AdminUserSummary>,
		total: String = r.total.to_string(),
	}
}

/// A live user balance (the user-detail drawer).
#[derive(Serialize)]
pub struct UserBalance {
	pub amount: String,
	pub pending: String,
	pub authoritative: bool,
	pub as_of: String,
}

impl From<bk::UserBalanceResponse> for UserBalance {
	fn from(b: bk::UserBalanceResponse) -> Self {
		Self {
			amount: b.amount,
			pending: b.pending,
			authoritative: b.authoritative,
			as_of: b.as_of.to_string(),
		}
	}
}

/// Per-rail liquidity plus the operator funding view (`treasury_*`/`onchain_*` are
/// best-effort chain reads — empty means the rail is unconfigured or the read was
/// unavailable, never an error).
#[derive(Serialize)]
pub struct RailLiquidity {
	pub network: String,
	pub custody: String,
	pub treasury_address: String,
	pub onchain_usdt: String,
	pub onchain_gas: String,
	/// The rail's sweep gas-station wallet — fund native coin here (never USDT).
	pub gas_station_address: String,
	pub gas_station_gas: String,
	/// Whether the addresses above are testnet-tagged (TON renders a different friendly
	/// address per realm); false on the rails whose address form is network-agnostic.
	pub is_testnet: bool,
}

/// The two-layer treasury picture (Treasury screen).
#[derive(Serialize)]
pub struct Treasury {
	pub rails: Vec<RailLiquidity>,
	pub bank: String,
	pub total_custody: String,
	pub fund_capital: String,
	pub fee_revenue: String,
	pub held_for_clients: String,
	pub reserved_for_withdrawals: String,
}

impl From<bk::Treasury> for Treasury {
	fn from(t: bk::Treasury) -> Self {
		Self {
			rails: t
				.rails
				.into_iter()
				.map(|r| RailLiquidity {
					network: r.network,
					custody: r.custody,
					treasury_address: r.treasury_address,
					onchain_usdt: r.onchain_usdt,
					onchain_gas: r.onchain_gas,
					gas_station_address: r.gas_station_address,
					gas_station_gas: r.gas_station_gas,
					is_testnet: r.is_testnet,
				})
				.collect(),
			bank: t.bank,
			total_custody: t.total_custody,
			fund_capital: t.fund_capital,
			fee_revenue: t.fee_revenue,
			held_for_clients: t.held_for_clients,
			reserved_for_withdrawals: t.reserved_for_withdrawals,
		}
	}
}

/// One outbox row the relay parked — the "money didn't move" set (Overview screen).
/// `reason` is the relay's last error; a `compensated` row already ran its recovery and
/// must never be unparked (the hub refuses).
#[derive(Serialize)]
pub struct ParkedEvent {
	pub seq: String,
	pub event_id: String,
	pub aggregate: String,
	pub aggregate_id: String,
	pub kind: String,
	pub reason: String,
	pub parked_at: String,
	pub compensated: bool,
}

impl From<bk::ParkedEvent> for ParkedEvent {
	fn from(e: bk::ParkedEvent) -> Self {
		Self {
			seq: e.seq.to_string(),
			event_id: e.event_id,
			aggregate: e.aggregate,
			aggregate_id: e.aggregate_id,
			kind: e.kind,
			reason: e.reason,
			parked_at: e.parked_at.to_string(),
			compensated: e.compensated,
		}
	}
}

list_dto! { ParkedEventList from bk::ParkedEventList { events: Vec<ParkedEvent> } }

/// One queued redemption in the Valuation screen's queue.
#[derive(Serialize)]
pub struct RedemptionQueueItem {
	pub redemption_id: String,
	pub user_id: String,
	pub email: String,
	pub service: String,
	pub units: String,
	pub created_at: String,
}

#[derive(Serialize)]
pub struct RedemptionQueue {
	pub items: Vec<RedemptionQueueItem>,
}

impl From<bk::RedemptionQueue> for RedemptionQueue {
	fn from(q: bk::RedemptionQueue) -> Self {
		Self {
			items: q
				.items
				.into_iter()
				.map(|i| RedemptionQueueItem {
					redemption_id: i.redemption_id,
					user_id: i.user_id,
					email: i.email,
					service: i.service,
					units: i.units,
					created_at: i.created_at.to_string(),
				})
				.collect(),
		}
	}
}

/// One withdrawal awaiting operator action (admin Withdrawals screen). `source` is
/// `user` or `revenue`; a revenue payout is the fund's own money leaving, so it carries
/// no `user_id`/`email` and the screen labels it rather than showing a blank investor.
#[derive(Serialize)]
pub struct WithdrawalQueueItem {
	pub withdrawal_id: String,
	pub source: String,
	pub user_id: String,
	pub email: String,
	pub network: String,
	pub address: String,
	pub amount: String,
	pub net_amount: String,
	pub state: String,
	pub created_at: String,
}

#[derive(Serialize)]
pub struct WithdrawalQueue {
	pub items: Vec<WithdrawalQueueItem>,
}

impl From<bk::WithdrawalQueue> for WithdrawalQueue {
	fn from(q: bk::WithdrawalQueue) -> Self {
		Self {
			items: q
				.items
				.into_iter()
				.map(|i| WithdrawalQueueItem {
					withdrawal_id: i.withdrawal_id,
					source: i.source,
					user_id: i.user_id,
					email: i.email,
					network: i.network,
					address: i.address,
					amount: i.amount,
					net_amount: i.net_amount,
					state: i.state,
					created_at: i.created_at.to_string(),
				})
				.collect(),
		}
	}
}

/// Per-rail payout options (admin Revenue screen) — the mirror of a user's
/// `NetworkWithdrawable`.
#[derive(Serialize)]
pub struct RevenueRail {
	pub network: String,
	pub payable: String,
	pub instant: String,
	pub minimum: String,
}

/// What the fund has EARNED and may pay itself: the `fee` claim, credited by the fee
/// retained on a user withdrawal and by the settled 2-and-20. Client money and the
/// fund's seed capital are separate claims and are not part of this figure.
#[derive(Serialize)]
pub struct FundRevenue {
	pub earned: String,
	pub available: String,
	pub pending_payout: String,
	pub rails: Vec<RevenueRail>,
}

impl From<bk::FundRevenue> for FundRevenue {
	fn from(r: bk::FundRevenue) -> Self {
		Self {
			earned: r.earned,
			available: r.available,
			pending_payout: r.pending_payout,
			rails: r
				.rails
				.into_iter()
				.map(|rail| RevenueRail {
					network: rail.network,
					payable: rail.payable,
					instant: rail.instant,
					minimum: rail.minimum,
				})
				.collect(),
		}
	}
}

/// The money-plane read-only kill-switch state (Cabinet screen).
#[derive(Serialize)]
pub struct OperationsMode {
	pub read_only: bool,
}

impl From<bk::OperationsMode> for OperationsMode {
	fn from(m: bk::OperationsMode) -> Self {
		Self { read_only: m.read_only }
	}
}

#[derive(Serialize)]
pub struct FeatureFlag {
	pub key: String,
	pub description: String,
	pub enabled: bool,
	pub rollout: u32,
}

/// The platform/cabinet config (Cabinet screen: maintenance, announcement, flags).
#[derive(Serialize)]
pub struct PlatformConfig {
	pub maintenance_mode: bool,
	pub announcement_title: String,
	pub announcement_body: String,
	pub announcement_active: bool,
	pub flags: Vec<FeatureFlag>,
}

impl From<cc::PlatformConfig> for PlatformConfig {
	fn from(c: cc::PlatformConfig) -> Self {
		Self {
			maintenance_mode: c.maintenance_mode,
			announcement_title: c.announcement_title,
			announcement_body: c.announcement_body,
			announcement_active: c.announcement_active,
			flags: c
				.flags
				.into_iter()
				.map(|f| FeatureFlag {
					key: f.key,
					description: f.description,
					enabled: f.enabled,
					rollout: f.rollout,
				})
				.collect(),
		}
	}
}

/// One inbox entry. Unix-second timestamps are rendered as strings for the same
/// reason every other i64 here is: the old proto-loader emitted `longs: String`, and
/// the frontend's generated types still expect that shape.
#[derive(serde::Serialize)]
pub struct Notification {
	pub id: String,
	pub topic: String,
	pub kind: String,
	pub title: String,
	pub body: String,
	pub link: String,
	pub occurred_at: String,
	pub created_at: String,
	/// "0" while unread.
	pub read_at: String,
}

list_dto! {
	NotificationList from cc::ListNotificationsResponse as r {
		notifications: Vec<Notification>,
		next_cursor: String = r.next_cursor,
		unread_count: u32 = r.unread_count,
	}
}

#[derive(serde::Serialize)]
pub struct UnreadCount {
	pub unread_count: u32,
}

#[derive(serde::Serialize)]
pub struct MarkReadResult {
	pub marked: u32,
	pub unread_count: u32,
}

#[derive(serde::Serialize)]
pub struct TopicSubscription {
	pub topic: String,
	pub label: String,
	pub description: String,
	pub subscribed: bool,
	pub email_enabled: bool,
}

/// The whole delivery surface. Both master switches may be false at once — that is a
/// supported state, so the frontend must not treat it as a broken record.
#[derive(serde::Serialize)]
pub struct NotificationSettings {
	pub in_app_enabled: bool,
	pub email_enabled: bool,
	pub email: String,
	pub email_verified: bool,
	pub topics: Vec<TopicSubscription>,
}

impl From<cc::Notification> for Notification {
	fn from(n: cc::Notification) -> Self {
		Self {
			id: n.id,
			topic: n.topic,
			kind: n.kind,
			title: n.title,
			body: n.body,
			link: n.link,
			occurred_at: n.occurred_at.to_string(),
			created_at: n.created_at.to_string(),
			read_at: n.read_at.to_string(),
		}
	}
}

impl From<cc::GetUnreadCountResponse> for UnreadCount {
	fn from(r: cc::GetUnreadCountResponse) -> Self {
		Self { unread_count: r.unread_count }
	}
}

impl From<cc::MarkReadResponse> for MarkReadResult {
	fn from(r: cc::MarkReadResponse) -> Self {
		Self {
			marked: r.marked,
			unread_count: r.unread_count,
		}
	}
}

impl From<cc::NotificationSettings> for NotificationSettings {
	fn from(s: cc::NotificationSettings) -> Self {
		Self {
			in_app_enabled: s.in_app_enabled,
			email_enabled: s.email_enabled,
			email: s.email,
			email_verified: s.email_verified,
			topics: s
				.topics
				.into_iter()
				.map(|t| TopicSubscription {
					topic: t.topic,
					label: t.label,
					description: t.description,
					subscribed: t.subscribed,
					email_enabled: t.email_enabled,
				})
				.collect(),
		}
	}
}

// ── consilium: multi-owner authorization (money plane) ───────────────────────

/// A proto enum on the browser wire: the generated `as_str_name()` minus its
/// `SCREAMING_PREFIX_`, lowercased. Derived rather than hand-matched so a variant added
/// to the proto reaches the browser as itself instead of falling through a stale arm.
fn enum_label(name: &str, prefix: &str) -> String {
	name.strip_prefix(prefix).unwrap_or(name).to_ascii_lowercase()
}

/// `ada@example.com` → `a***@example.com`. Applied on every redacted surface a non-owner
/// can reach; idempotent, so re-masking what the plane already masked is safe and this
/// stays a second line of defense rather than a substitute for the plane's own. A value
/// with no `@`, or with no local part, is replaced wholesale — something that is not an
/// address is not something to reveal a prefix of.
pub fn mask_email(email: &str) -> String {
	if email.is_empty() {
		return String::new();
	}
	match email.split_once('@') {
		// `chars().next()` and not `&local[..1]`: a non-ASCII local part would put that
		// slice inside a UTF-8 character and panic.
		Some((local, domain)) if !domain.is_empty() => match local.chars().next() {
			Some(first) => format!("{first}***@{domain}"),
			None => "***".to_string(),
		},
		_ => "***".to_string(),
	}
}

/// The immutable subject of a revenue payout. The address is carried in FULL — a
/// truncated address on a surface where a human approves it is an invitation to approve
/// the wrong one.
#[derive(Default, Serialize)]
pub struct RevenuePayoutTerms {
	pub network: String,
	pub address: String,
	pub amount: String,
	pub memo: String,
}

impl From<bk::RevenuePayoutTerms> for RevenuePayoutTerms {
	fn from(t: bk::RevenuePayoutTerms) -> Self {
		Self {
			network: t.network,
			address: t.address,
			amount: t.amount,
			memo: t.memo,
		}
	}
}

#[derive(Serialize)]
pub struct ConsiliumVoter {
	pub user_id: String,
	pub email: String,
	pub decision: String,
	pub decided_at: String,
	pub notified: bool,
}

impl From<bk::ConsiliumVoter> for ConsiliumVoter {
	fn from(v: bk::ConsiliumVoter) -> Self {
		let decision = enum_label(v.decision().as_str_name(), "VOTE_DECISION_");
		Self {
			user_id: v.user_id,
			email: v.email,
			decision,
			decided_at: v.decided_at.to_string(),
			notified: v.notified,
		}
	}
}

/// One consilium in full — the owner-only view, with the per-voter breakdown.
#[derive(Serialize)]
pub struct Consilium {
	pub id: String,
	pub state: String,
	pub revenue_payout: RevenuePayoutTerms,
	pub payload_hash: String,
	pub initiator_user_id: String,
	pub initiator_email: String,
	pub owner_count: u32,
	pub threshold: u32,
	pub approvals: u32,
	pub rejections: u32,
	pub voters: Vec<ConsiliumVoter>,
	pub created_at: String,
	pub expires_at: String,
	pub decided_at: String,
	pub executed_withdrawal_id: String,
	pub failure_reason: String,
	/// Monotonic per consilium. The live page watches this and refetches when it moves.
	pub version: String,
}

impl From<bk::Consilium> for Consilium {
	fn from(c: bk::Consilium) -> Self {
		let state = enum_label(c.state().as_str_name(), "CONSILIUM_STATE_");
		Self {
			id: c.id,
			state,
			revenue_payout: c.revenue_payout.map(RevenuePayoutTerms::from).unwrap_or_default(),
			payload_hash: c.payload_hash,
			initiator_user_id: c.initiator_user_id,
			initiator_email: c.initiator_email,
			owner_count: c.owner_count,
			threshold: c.threshold,
			approvals: c.approvals,
			rejections: c.rejections,
			voters: c.voters.into_iter().map(ConsiliumVoter::from).collect(),
			created_at: c.created_at.to_string(),
			expires_at: c.expires_at.to_string(),
			decided_at: c.decided_at.to_string(),
			executed_withdrawal_id: c.executed_withdrawal_id,
			failure_reason: c.failure_reason,
			version: c.version.to_string(),
		}
	}
}

list_dto! { ConsiliumList from bk::ConsiliumList { items: Vec<Consilium> } }

/// What the emailed owner is shown before voting. Deliberately narrower than
/// [`Consilium`]: no other owner's identity and no vote-by-vote breakdown — and both
/// addresses are masked, because this is the one surface reachable with no session.
#[derive(Default, Serialize)]
pub struct ConsiliumInvitation {
	pub consilium_id: String,
	pub state: String,
	pub revenue_payout: RevenuePayoutTerms,
	pub payload_hash: String,
	pub initiator_email: String,
	pub voter_email: String,
	pub threshold: u32,
	pub approvals: u32,
	pub owner_count: u32,
	pub created_at: String,
	pub expires_at: String,
	pub decision: String,
	pub attempts_remaining: u32,
}

impl From<bk::ConsiliumInvitation> for ConsiliumInvitation {
	fn from(i: bk::ConsiliumInvitation) -> Self {
		let state = enum_label(i.state().as_str_name(), "CONSILIUM_STATE_");
		let decision = enum_label(i.decision().as_str_name(), "VOTE_DECISION_");
		Self {
			consilium_id: i.consilium_id,
			state,
			revenue_payout: i.revenue_payout.map(RevenuePayoutTerms::from).unwrap_or_default(),
			payload_hash: i.payload_hash,
			initiator_email: mask_email(&i.initiator_email),
			voter_email: mask_email(&i.voter_email),
			threshold: i.threshold,
			approvals: i.approvals,
			owner_count: i.owner_count,
			created_at: i.created_at.to_string(),
			expires_at: i.expires_at.to_string(),
			decision,
			attempts_remaining: i.attempts_remaining,
		}
	}
}

/// The invitation as it stands after a vote, plus whether this vote carried the verdict.
#[derive(Serialize)]
pub struct ConsiliumDecision {
	pub invitation: ConsiliumInvitation,
	pub decided: bool,
}

impl From<bk::SubmitDecisionResponse> for ConsiliumDecision {
	fn from(r: bk::SubmitDecisionResponse) -> Self {
		Self {
			invitation: r.invitation.map(ConsiliumInvitation::from).unwrap_or_default(),
			decided: r.decided,
		}
	}
}

// ── governance: the roster, its removals and its admissions (ownership plane) ─
//
// A seat changes hands only through a consilium in this plane. Removal passes when the
// target accepts from their mailbox OR every eligible peer votes to remove; admission
// passes only on unanimity of every other owner, which is what stops a minority minting
// sock puppets into a majority before opening a payout.

#[derive(Serialize)]
pub struct Owner {
	pub user_id: String,
	pub email: String,
	pub display_name: String,
	pub owner_since: String,
}

impl From<cc::Owner> for Owner {
	fn from(o: cc::Owner) -> Self {
		Self {
			user_id: o.user_id,
			email: o.email,
			display_name: o.display_name,
			owner_since: o.owner_since.to_string(),
		}
	}
}

list_dto! {
	OwnerList from cc::OwnerList as l {
		items: Vec<Owner>,
		/// True below THREE owners, where the money plane's `floor(N/2)+1` threshold is
		/// unreachable and no payout can be authorized. Distinct from the removal floor,
		/// which is two: two owners is a recoverable pause, because two can still admit a
		/// third, whereas a floor of three would make a bad actor unremovable.
		below_payout_floor: bool = l.below_payout_floor,
	}
}

#[derive(Serialize)]
pub struct RemovalPeer {
	pub user_id: String,
	pub email: String,
	pub vote: String,
	pub voted_at: String,
}

impl From<cc::RemovalPeer> for RemovalPeer {
	fn from(p: cc::RemovalPeer) -> Self {
		let vote = enum_label(p.vote().as_str_name(), "REMOVAL_VOTE_");
		Self {
			user_id: p.user_id,
			email: p.email,
			vote,
			voted_at: p.voted_at.to_string(),
		}
	}
}

#[derive(Serialize)]
pub struct OwnerRemoval {
	pub id: String,
	pub state: String,
	pub target_user_id: String,
	pub target_email: String,
	pub initiator_user_id: String,
	pub initiator_email: String,
	pub reason: String,
	/// Every owner except the target and the initiator. Empty means peer unanimity is
	/// unavailable and only the target's own acceptance can carry this.
	pub peers: Vec<RemovalPeer>,
	pub target_decision: String,
	pub target_decided_at: String,
	pub target_notified: bool,
	pub owner_count: u32,
	pub created_at: String,
	pub expires_at: String,
	pub decided_at: String,
	pub void_reason: String,
	pub version: String,
}

impl From<cc::OwnerRemoval> for OwnerRemoval {
	fn from(r: cc::OwnerRemoval) -> Self {
		let state = enum_label(r.state().as_str_name(), "OWNER_REMOVAL_STATE_");
		let target_decision = enum_label(r.target_decision().as_str_name(), "REMOVAL_VOTE_");
		Self {
			id: r.id,
			state,
			target_user_id: r.target_user_id,
			target_email: r.target_email,
			initiator_user_id: r.initiator_user_id,
			initiator_email: r.initiator_email,
			reason: r.reason,
			peers: r.peers.into_iter().map(RemovalPeer::from).collect(),
			target_decision,
			target_decided_at: r.target_decided_at.to_string(),
			target_notified: r.target_notified,
			owner_count: r.owner_count,
			created_at: r.created_at.to_string(),
			expires_at: r.expires_at.to_string(),
			decided_at: r.decided_at.to_string(),
			void_reason: r.void_reason,
			version: r.version.to_string(),
		}
	}
}

list_dto! { OwnerRemovalList from cc::OwnerRemovalList { items: Vec<OwnerRemoval> } }

#[derive(Serialize)]
pub struct AdmissionPeer {
	pub user_id: String,
	pub email: String,
	pub vote: String,
	pub voted_at: String,
}

impl From<cc::AdmissionPeer> for AdmissionPeer {
	fn from(p: cc::AdmissionPeer) -> Self {
		let vote = enum_label(p.vote().as_str_name(), "ADMISSION_VOTE_");
		Self {
			user_id: p.user_id,
			email: p.email,
			vote,
			voted_at: p.voted_at.to_string(),
		}
	}
}

/// Granting a seat. The shape differs from [`OwnerRemoval`] by more than a rename: there
/// is no target-decision trio, because the candidate does not vote on their own admission
/// — they are not an owner yet.
#[derive(Serialize)]
pub struct OwnerAdmission {
	pub id: String,
	pub state: String,
	pub candidate_user_id: String,
	pub candidate_email: String,
	pub initiator_user_id: String,
	pub initiator_email: String,
	pub reason: String,
	/// Every owner except the initiator. Never empty: an admission with nobody to agree
	/// is refused at open rather than left open and unpassable.
	pub peers: Vec<AdmissionPeer>,
	pub owner_count: u32,
	pub created_at: String,
	pub expires_at: String,
	pub decided_at: String,
	pub void_reason: String,
	pub version: String,
}

impl From<cc::OwnerAdmission> for OwnerAdmission {
	fn from(a: cc::OwnerAdmission) -> Self {
		let state = enum_label(a.state().as_str_name(), "OWNER_ADMISSION_STATE_");
		Self {
			id: a.id,
			state,
			candidate_user_id: a.candidate_user_id,
			candidate_email: a.candidate_email,
			initiator_user_id: a.initiator_user_id,
			initiator_email: a.initiator_email,
			reason: a.reason,
			peers: a.peers.into_iter().map(AdmissionPeer::from).collect(),
			owner_count: a.owner_count,
			created_at: a.created_at.to_string(),
			expires_at: a.expires_at.to_string(),
			decided_at: a.decided_at.to_string(),
			void_reason: a.void_reason,
			version: a.version.to_string(),
		}
	}
}

list_dto! { OwnerAdmissionList from cc::OwnerAdmissionList { items: Vec<OwnerAdmission> } }

/// What the TARGET is shown before answering: no peer identities, no vote breakdown.
/// Both addresses are masked — this surface, too, needs no session.
#[derive(Default, Serialize)]
pub struct OwnerRemovalInvitation {
	pub removal_id: String,
	pub state: String,
	pub initiator_email: String,
	pub target_email: String,
	pub reason: String,
	pub created_at: String,
	pub expires_at: String,
	pub decision: String,
	pub attempts_remaining: u32,
}

impl From<cc::OwnerRemovalInvitation> for OwnerRemovalInvitation {
	fn from(i: cc::OwnerRemovalInvitation) -> Self {
		let state = enum_label(i.state().as_str_name(), "OWNER_REMOVAL_STATE_");
		let decision = enum_label(i.decision().as_str_name(), "REMOVAL_VOTE_");
		Self {
			removal_id: i.removal_id,
			state,
			initiator_email: mask_email(&i.initiator_email),
			target_email: mask_email(&i.target_email),
			reason: i.reason,
			created_at: i.created_at.to_string(),
			expires_at: i.expires_at.to_string(),
			decision,
			attempts_remaining: i.attempts_remaining,
		}
	}
}

#[derive(Serialize)]
pub struct RemovalDecision {
	pub invitation: OwnerRemovalInvitation,
	pub decided: bool,
}

impl From<cc::SubmitSelfDecisionResponse> for RemovalDecision {
	fn from(r: cc::SubmitSelfDecisionResponse) -> Self {
		Self {
			invitation: r.invitation.map(OwnerRemovalInvitation::from).unwrap_or_default(),
			decided: r.decided,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn masking_keeps_one_character_and_is_idempotent() {
		assert_eq!(mask_email("ada@example.com"), "a***@example.com");
		assert_eq!(mask_email("a***@example.com"), "a***@example.com");
		assert_eq!(mask_email(""), "");
		assert_eq!(mask_email("not-an-address"), "***");
		assert_eq!(mask_email("@example.com"), "***");
		assert_eq!(mask_email("ada@"), "***");
	}

	/// A non-ASCII local part must not be sliced mid-character — a byte slice would panic
	/// on one, and a panic inside a handler is a 500 on a page a stranger can reach.
	#[test]
	fn masking_survives_a_non_ascii_local_part() {
		assert_eq!(mask_email("ада@example.com"), "а***@example.com");
	}

	/// The browser vocabulary is derived from the proto, so a state added upstream
	/// arrives as itself rather than through a stale hand-written arm.
	#[test]
	fn enum_labels_are_the_proto_name_without_its_prefix() {
		assert_eq!(enum_label(bk::ConsiliumState::Open.as_str_name(), "CONSILIUM_STATE_"), "open");
		assert_eq!(enum_label(bk::ConsiliumState::ExecutionFailed.as_str_name(), "CONSILIUM_STATE_"), "execution_failed");
		assert_eq!(enum_label(bk::VoteDecision::Approve.as_str_name(), "VOTE_DECISION_"), "approve");
	}

	/// The redacted invitation is the one surface a stranger holding a link can read: no
	/// address on it may be renderable back to a real mailbox.
	#[test]
	fn the_public_invitation_masks_both_addresses() {
		let invitation = ConsiliumInvitation::from(bk::ConsiliumInvitation {
			initiator_email: "ada@example.com".into(),
			voter_email: "grace@example.com".into(),
			..Default::default()
		});
		assert_eq!(invitation.initiator_email, "a***@example.com");
		assert_eq!(invitation.voter_email, "g***@example.com");
	}
}

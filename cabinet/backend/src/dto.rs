//! Browser-facing JSON DTOs. These reproduce the exact wire shape the old TS BFF
//! emitted (proto-loader with `keepCase` + `longs: String`): snake_case fields, with
//! 64-bit integers rendered as strings so the committed `shared/contracts/gen` types
//! stay valid and the React fetch code is unchanged.

use evbanking_contracts::banking::v1 as bk;
use evconcierge_contracts::concierge::v1 as cc;
use serde::Serialize;

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

#[derive(Serialize)]
pub struct WithdrawalList {
	pub withdrawals: Vec<Withdrawal>,
}

impl From<bk::WithdrawalList> for WithdrawalList {
	fn from(l: bk::WithdrawalList) -> Self {
		Self {
			withdrawals: l.withdrawals.into_iter().map(Withdrawal::from).collect(),
		}
	}
}

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

#[derive(Serialize)]
pub struct DepositList {
	pub deposits: Vec<Deposit>,
}

impl From<bk::DepositList> for DepositList {
	fn from(l: bk::DepositList) -> Self {
		Self {
			deposits: l.deposits.into_iter().map(Deposit::from).collect(),
		}
	}
}

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

#[derive(Serialize)]
pub struct OperationList {
	pub operations: Vec<Operation>,
	/// True when the history is longer than the page returned.
	pub truncated: bool,
}

impl From<bk::OperationList> for OperationList {
	fn from(l: bk::OperationList) -> Self {
		Self {
			operations: l.operations.into_iter().map(Operation::from).collect(),
			truncated: l.truncated,
		}
	}
}

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

#[derive(Serialize)]
pub struct AllocationList {
	pub allocations: Vec<Allocation>,
}

impl From<bk::AllocationList> for AllocationList {
	fn from(l: bk::AllocationList) -> Self {
		Self {
			allocations: l.allocations.into_iter().map(Allocation::from).collect(),
		}
	}
}

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

#[derive(Serialize)]
pub struct PositionList {
	pub positions: Vec<Position>,
}

impl From<bk::PositionList> for PositionList {
	fn from(l: bk::PositionList) -> Self {
		Self {
			positions: l.positions.into_iter().map(Position::from).collect(),
		}
	}
}

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

#[derive(Serialize)]
pub struct RedemptionList {
	pub redemptions: Vec<Redemption>,
}

impl From<bk::RedemptionList> for RedemptionList {
	fn from(l: bk::RedemptionList) -> Self {
		Self {
			redemptions: l.redemptions.into_iter().map(Redemption::from).collect(),
		}
	}
}

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

#[derive(Serialize)]
pub struct AdminUserList {
	pub users: Vec<AdminUserSummary>,
	pub total: String,
}

impl From<cc::ListUsersResponse> for AdminUserList {
	fn from(r: cc::ListUsersResponse) -> Self {
		Self {
			users: r.users.into_iter().map(AdminUserSummary::from).collect(),
			total: r.total.to_string(),
		}
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

#[derive(Serialize)]
pub struct ParkedEventList {
	pub events: Vec<ParkedEvent>,
}

impl From<bk::ParkedEventList> for ParkedEventList {
	fn from(l: bk::ParkedEventList) -> Self {
		Self {
			events: l.events.into_iter().map(ParkedEvent::from).collect(),
		}
	}
}

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

#[derive(serde::Serialize)]
pub struct NotificationList {
	pub notifications: Vec<Notification>,
	pub next_cursor: String,
	pub unread_count: u32,
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

impl From<cc::ListNotificationsResponse> for NotificationList {
	fn from(r: cc::ListNotificationsResponse) -> Self {
		Self {
			notifications: r.notifications.into_iter().map(Into::into).collect(),
			next_cursor: r.next_cursor,
			unread_count: r.unread_count,
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

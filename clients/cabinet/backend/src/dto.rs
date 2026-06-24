//! Browser-facing JSON DTOs. These reproduce the exact wire shape the old TS BFF
//! emitted (proto-loader with `keepCase` + `longs: String`): snake_case fields, with
//! 64-bit integers rendered as strings so the committed `shared/contracts/gen` types
//! stay valid and the React fetch code is unchanged.

use evbanking_contracts::banking::v1 as bk;
use evconcierge_contracts::concierge::v1 as cc;
use serde::Serialize;

use crate::session::User;

// ── /api/auth/session ────────────────────────────────────────────────────────
// Note the camelCase principal: the old BFF mapped the proto `user_id` → `userId`
// for this endpoint only (the session principal), unlike the snake_case passthroughs.

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUser {
	pub user_id: String,
	pub email: String,
	pub status: String,
}

#[derive(Serialize)]
pub struct SessionInfo {
	pub authenticated: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub user: Option<SessionUser>,
}

impl SessionInfo {
	pub fn authenticated(user: User) -> Self {
		Self {
			authenticated: true,
			user: Some(SessionUser {
				user_id: user.user_id,
				email: user.email,
				status: user.status,
			}),
		}
	}

	pub fn anonymous() -> Self {
		Self { authenticated: false, user: None }
	}
}

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
		}
	}
}

#[derive(Serialize)]
pub struct Session {
	pub id: String,
	pub user_agent: String,
	pub ip: String,
	pub created_at: String,
	pub last_seen: String,
	pub current: bool,
}

impl From<cc::Session> for Session {
	fn from(s: cc::Session) -> Self {
		Self {
			id: s.id,
			user_agent: s.user_agent,
			ip: s.ip,
			created_at: s.created_at.to_string(),
			last_seen: s.last_seen.to_string(),
			current: s.current,
		}
	}
}

#[derive(Serialize)]
pub struct SessionList {
	pub sessions: Vec<Session>,
}

impl From<cc::ListSessionsResponse> for SessionList {
	fn from(r: cc::ListSessionsResponse) -> Self {
		Self {
			sessions: r.sessions.into_iter().map(Session::from).collect(),
		}
	}
}

// ── piggybank: wallet ────────────────────────────────────────────────────────

#[derive(Serialize, Default)]
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
}

impl From<bk::DepositAddress> for DepositAddress {
	fn from(d: bk::DepositAddress) -> Self {
		Self {
			network: d.network,
			address: d.address,
			min_confirmations: d.min_confirmations,
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
		}
	}
}

#[cfg(test)]
mod tests {
	use serde_json::json;

	use super::SessionInfo;
	use crate::session::User;

	// `/api/auth/session` is the only endpoint with a camelCase principal (`userId`), and
	// `user` is omitted entirely when anonymous. The browser's who-am-I (sidebar) depends on
	// exactly this shape, so pin it.
	#[test]
	fn session_authenticated_shape() {
		let user = User {
			user_id: "u-1".into(),
			email: "a@b.c".into(),
			status: "active".into(),
		};
		let got = serde_json::to_value(SessionInfo::authenticated(user)).unwrap();
		assert_eq!(
			got,
			json!({
				"authenticated": true,
				"user": { "userId": "u-1", "email": "a@b.c", "status": "active" }
			})
		);
	}

	#[test]
	fn session_anonymous_omits_user() {
		let got = serde_json::to_value(SessionInfo::anonymous()).unwrap();
		assert_eq!(got, json!({ "authenticated": false }));
	}
}

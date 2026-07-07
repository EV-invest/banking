//! Cross-cutting authorization — the shared role vocabulary and the money plane's
//! permission matrix.
//!
//! [`Role`] is OWNED by the identity plane (concierge); banking receives it over the
//! one-way user-lifecycle bridge (only the string crosses) and mirrors it onto the
//! local user projection. The four discriminant strings are a **cross-plane contract**
//! — keep them byte-identical with concierge's `domain::authz::Role`
//! ([`role_strings_are_canonical`] guards this side).
//!
//! [`Permission`] is **local** to the money plane: banking enforces money/treasury
//! permissions; identity/platform permissions live in concierge's own `Permission`.
//! [`grants`] is the pure policy (the RBAC "matrix") — the single place the matrix is
//! defined, carrying the separation-of-duties intent (view ≠ move money).

use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// The platform-wide user role, ordered least→most privileged. Mirrored from the
/// identity plane; `Investor` is the default for any user banking hasn't been told
/// otherwise about.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
	#[default]
	Investor,
	Operator,
	Admin,
	Owner,
}

impl Role {
	/// The stored/wire discriminant. Cross-plane bridge contract — do not diverge from
	/// concierge's `Role::as_str`.
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Investor => "investor",
			Self::Operator => "operator",
			Self::Admin => "admin",
			Self::Owner => "owner",
		}
	}

	/// Parse the stored/bridged form. An unrecognized value is a validation error
	/// rather than a silent default, so a corrupt row never quietly grants privilege.
	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"investor" => Ok(Self::Investor),
			"operator" => Ok(Self::Operator),
			"admin" => Ok(Self::Admin),
			"owner" => Ok(Self::Owner),
			other => Err(DomainError::Validation(format!("unknown role: {other}"))),
		}
	}

	/// Tolerant parse for the bridge: an empty/unknown value from an older concierge
	/// (pre-role rows carry no role) is treated as `Investor` rather than failing the
	/// event. Distinct from [`Role::parse`], which is strict for a persisted local row.
	pub fn parse_or_default(raw: &str) -> Self {
		Self::parse(raw).unwrap_or_default()
	}
}

/// A capability in the MONEY plane. Identity/platform capabilities live in concierge's
/// own `Permission` — the sets are deliberately disjoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Permission {
	/// Read the treasury / chart-of-accounts aggregate.
	TreasuryRead,
	/// Read any user's balance/wallet (the operator user-detail drawer).
	UserBalanceRead,
	/// Post a fund NAV valuation.
	ValuationPost,
	/// Settle a queued redemption.
	RedemptionSettle,
	/// Fail (void + refund) a queued redemption.
	RedemptionFail,
	/// Mark a queued withdrawal dispatched (broadcast).
	WithdrawalDispatch,
	/// Mark a dispatched withdrawal settled.
	WithdrawalSettle,
	/// Fail a withdrawal.
	WithdrawalFail,
	/// Seed fund capital / record an off-rail deposit.
	CapitalManage,
	/// Toggle the money-plane operations mode (read-only kill-switch).
	OperationsManage,
	/// Unpark a parked outbox event so the relay re-drives it.
	OutboxManage,
	/// Revoke a user's money-plane tokens (banking's own defense-in-depth revoke;
	/// identity-plane session revocation is concierge's, mirrored via the bridge).
	UserRevoke,
	/// Disable a user's money-plane account directly (independent of the bridge freeze).
	UserSuspend,
	/// Supersede a user's PROVABLY DEAD deposit-address key (a KEK-epoch casualty —
	/// the signer can no longer unseal it) with a freshly minted keypair. Recovery
	/// only: the signer refuses to rotate a healthy key.
	DepositAddressRotate,
}

/// The role→permission policy (pure). The money-plane RBAC matrix, read as separation
/// of duties:
/// - `Investor` holds nothing (no console).
/// - `Operator` may READ (treasury, any user balance) but move no money.
/// - `Admin` and `Owner` hold every money capability (role-granting is the identity
///   plane's concern, so the two are equivalent here).
pub fn grants(role: Role, permission: Permission) -> bool {
	use Permission::*;
	use Role::*;
	match role {
		Investor => false,
		Operator => matches!(permission, TreasuryRead | UserBalanceRead),
		Admin | Owner => true,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn role_strings_are_canonical() {
		// Cross-plane bridge contract: these four strings must match concierge's Role
		// verbatim. If you change one, change concierge's `domain::authz::Role` too.
		assert_eq!(Role::Investor.as_str(), "investor");
		assert_eq!(Role::Operator.as_str(), "operator");
		assert_eq!(Role::Admin.as_str(), "admin");
		assert_eq!(Role::Owner.as_str(), "owner");
	}

	#[test]
	fn role_round_trips_and_defaults_tolerantly() {
		for role in [Role::Investor, Role::Operator, Role::Admin, Role::Owner] {
			assert_eq!(Role::parse(role.as_str()).unwrap(), role);
		}
		assert!(Role::parse("root").is_err());
		// Bridge tolerance: an empty/unknown role degrades to Investor.
		assert_eq!(Role::parse_or_default(""), Role::Investor);
		assert_eq!(Role::parse_or_default("root"), Role::Investor);
	}

	#[test]
	fn matrix_enforces_view_versus_move() {
		// Operator reads, never moves money.
		assert!(grants(Role::Operator, Permission::TreasuryRead));
		assert!(grants(Role::Operator, Permission::UserBalanceRead));
		assert!(!grants(Role::Operator, Permission::ValuationPost));
		assert!(!grants(Role::Operator, Permission::RedemptionSettle));
		assert!(!grants(Role::Operator, Permission::OutboxManage));
		// Admin/Owner move money.
		assert!(grants(Role::Admin, Permission::ValuationPost));
		assert!(grants(Role::Owner, Permission::WithdrawalSettle));
		assert!(grants(Role::Admin, Permission::OutboxManage));
		// Investor holds nothing.
		assert!(!grants(Role::Investor, Permission::TreasuryRead));
	}
}

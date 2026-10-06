//! The shared role vocabulary.
//!
//! [`Role`] is OWNED by the identity plane (concierge); banking receives it over the
//! one-way user-lifecycle bridge (only the string crosses) and mirrors it onto the
//! local user projection. The four discriminant strings are a **cross-plane contract**
//! — keep them byte-identical with concierge's `domain::authz::Role`
//! ([`role_strings_are_canonical`] guards this side).
//!
//! What a seat may do is not decided here: concierge resolves it to `bank:*` permissions
//! (`concierge_domain::authz::bank`), the bridge mirrors them, and the gates read those.

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
}

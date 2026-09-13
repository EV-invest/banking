use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
	#[error("{entity} not found: {id}")]
	NotFound { entity: &'static str, id: String },
	#[error("conflict: {0}")]
	Conflict(String),
	#[error("validation failed: {0}")]
	Validation(String),
	/// A domain policy forbids the action for this caller in principle (e.g. only the
	/// staking user may revoke, and only while the fund owns the allocation): retrying
	/// later changes nothing. Distinct from `Validation` (bad input) and from
	/// `Precondition` (a caller who IS entitled, refused by the current state) — it maps
	/// to gRPC `permission_denied`, not `invalid_argument` or `failed_precondition`.
	#[error("forbidden: {0}")]
	Forbidden(String),
	/// The system or the account is in a state that refuses the action *right now* — the
	/// outflow pause, a frozen account. The request is well-formed and the caller is
	/// entitled to make it; only the state stands in the way, and the same request goes
	/// through once the state changes. Distinct from `Forbidden` (this caller may never do
	/// this) — it maps to gRPC `failed_precondition`.
	#[error("precondition failed: {0}")]
	Precondition(String),
	/// Unexpected failure from a driven adapter (e.g. the database). Carries a
	/// description for logging only — it is never surfaced verbatim to clients,
	/// and an infrastructure failure must never be mapped to `Validation`.
	#[error("repository error: {0}")]
	Repository(String),
}

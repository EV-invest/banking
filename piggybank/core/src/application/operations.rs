//! Operations query use case — the caller's activity timeline.
//!
//! Read side only. The whole use case is clamping the requested page into the range
//! the read model will serve: the clamp lives here rather than in the adapter because
//! "how much history does a client get to ask for" is an application policy, and the
//! adapter should answer honestly for whatever it is handed.

use domain::{error::DomainError, users::UserId};

use crate::ports::operations::{DEFAULT_PAGE, MAX_PAGE, OperationFeed, OperationPage};

/// The caller's operations, newest first. `limit` of 0 means "unspecified" on the wire
/// (proto3 has no optional scalar presence here), which resolves to the default page;
/// anything above [`MAX_PAGE`] is clamped rather than refused — a client asking for
/// more history than exists wants the most it can have, not an error.
pub async fn list_operations(feed: &dyn OperationFeed, user: UserId, limit: u32) -> Result<OperationPage, DomainError> {
	let limit = match limit {
		0 => DEFAULT_PAGE,
		requested => requested.min(MAX_PAGE),
	};
	feed.list_by_user(user, limit).await
}

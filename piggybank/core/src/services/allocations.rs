//! `allocations` context — the registry of investable products.
//!
//! Reads are open to any authenticated user and filtered to what that user may see;
//! every write, and the unfiltered view of the catalog, is gated on
//! [`Permission::AllocationManage`] (Admin/Owner) — the same trust seam as posting a
//! valuation, because registering a product is what brings a fund into existence at
//! all, and opening it to an investor is what lets their money in.
//!
//! No money crosses this surface: the handlers below never touch the ledger or notify
//! the relay.
//!
//! `Result<_, Status>` is tonic's mandated handler signature; `Status` is a large
//! type we don't control, so the large-err lint does not apply in this module.
#![allow(clippy::result_large_err)]

use domain::{
	allocations::{AllocationAccess, AllocationIcon},
	authz::Permission,
	balance::ServiceId,
	money::Shares,
	users::UserId,
};
use evbanking_contracts::{
	allocation::{access as wire_access, state as wire_state},
	banking::v1::{self as pb, allocations_service_server::AllocationsService},
};
use tonic::{Request, Response, Status};

use crate::{
	AppState,
	application::allocations as allocations_app,
	ports::allocations::{AllocationAccessGrant, AllocationRecord},
	services::support::{caller_id, holds_permission, map_err, require_permission, resolve_target_user},
};

#[derive(Clone)]
pub struct AllocationsSvc {
	pub state: AppState,
}

impl AllocationsSvc {
	pub fn new(state: AppState) -> Self {
		Self { state }
	}

	/// The response every write handler ends on: the product re-read as the caller
	/// sees it. The aggregate a transition returns is deliberately clock-free and knows
	/// nothing of the caller's grant, so rather than answer with zeroed timestamps and a
	/// guessed `caller_access`, the handler pays one indexed read for an honest row —
	/// these are operator commands, not the hot path. Unrestricted: the caller just
	/// proved `AllocationManage`, so a product they hid from themselves still renders.
	async fn manager_view(&self, caller: UserId, service: &ServiceId) -> Result<Response<pb::Allocation>, Status> {
		let record = allocations_app::get_for(self.state.allocations.as_ref(), service, caller, true).await.map_err(map_err)?;
		Ok(Response::new(record_to_proto(&record)))
	}
}

#[tonic::async_trait]
impl AllocationsService for AllocationsSvc {
	async fn list_allocations(&self, request: Request<pb::ListAllocationsRequest>) -> Result<Response<pb::AllocationList>, Status> {
		let caller = caller_id(&request)?;
		let include_unlisted = request.get_ref().include_unlisted;
		// Refuse rather than silently downgrade to the visible list: a caller that asked
		// for drafts and got a filtered list would read it as "there are none".
		if include_unlisted {
			require_permission(&self.state, &request, Permission::AllocationManage).await?;
		}
		let records = allocations_app::list_for(self.state.allocations.as_ref(), caller, include_unlisted).await.map_err(map_err)?;
		Ok(Response::new(pb::AllocationList {
			allocations: records.iter().map(record_to_proto).collect(),
		}))
	}

	async fn get_allocation(&self, request: Request<pb::GetAllocationRequest>) -> Result<Response<pb::Allocation>, Status> {
		// Any authenticated user, any state — an investor holding units of a closed
		// product still has to render it. A product hidden from THIS caller is NOT_FOUND,
		// unless they hold AllocationManage: a question here, not a gate, because the
		// handler serves everyone and merely widens for a manager.
		let caller = caller_id(&request)?;
		let unrestricted = holds_permission(&self.state, &request, Permission::AllocationManage).await?;
		let service = ServiceId::parse(&request.get_ref().service).map_err(map_err)?;
		let record = allocations_app::get_for(self.state.allocations.as_ref(), &service, caller, unrestricted).await.map_err(map_err)?;
		Ok(Response::new(record_to_proto(&record)))
	}

	async fn register_allocation(&self, request: Request<pb::RegisterAllocationRequest>) -> Result<Response<pb::Allocation>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		let caller = caller_id(&request)?;
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		let icon = parse_icon(&req.icon)?;
		allocations_app::register(self.state.allocations.as_ref(), service.clone(), &req.title, &req.summary, icon)
			.await
			.map_err(map_err)?;
		self.manager_view(caller, &service).await
	}

	async fn update_allocation(&self, request: Request<pb::UpdateAllocationRequest>) -> Result<Response<pb::Allocation>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		let caller = caller_id(&request)?;
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		let icon = parse_icon_update(req.icon.as_deref())?;
		allocations_app::update_details(self.state.allocations.as_ref(), &service, &req.title, &req.summary, icon)
			.await
			.map_err(map_err)?;
		self.manager_view(caller, &service).await
	}

	async fn set_allocation_unit_cap(&self, request: Request<pb::SetAllocationUnitCapRequest>) -> Result<Response<pb::Allocation>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		let caller = caller_id(&request)?;
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		// Parsed at the boundary, so a malformed cap is an `invalid_argument` about the
		// input rather than a validation error from inside the aggregate.
		let unit_cap = Shares::parse_decimal(&req.unit_cap).map_err(map_err)?;
		allocations_app::set_unit_cap(self.state.allocations.as_ref(), &service, unit_cap).await.map_err(map_err)?;
		self.manager_view(caller, &service).await
	}

	async fn set_allocation_state(&self, request: Request<pb::SetAllocationStateRequest>) -> Result<Response<pb::Allocation>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		let caller = caller_id(&request)?;
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		match req.state.as_str() {
			wire_state::OPEN => allocations_app::open(self.state.allocations.as_ref(), &service).await,
			wire_state::CLOSED => allocations_app::close(self.state.allocations.as_ref(), &service).await,
			// `draft` is entered only by RegisterAllocation — a product that has taken
			// money must never be able to travel back to "never opened".
			other =>
				return Err(Status::invalid_argument(format!(
					"allocation state must be '{}' or '{}', got '{other}'",
					wire_state::OPEN,
					wire_state::CLOSED
				))),
		}
		.map_err(map_err)?;
		self.manager_view(caller, &service).await
	}

	async fn set_allocation_access(&self, request: Request<pb::SetAllocationAccessRequest>) -> Result<Response<pb::Allocation>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		let caller = caller_id(&request)?;
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		let access = parse_access(&req.access)?;
		allocations_app::set_access(self.state.allocations.as_ref(), &service, access).await.map_err(map_err)?;
		self.manager_view(caller, &service).await
	}

	async fn grant_allocation_access(&self, request: Request<pb::GrantAllocationAccessRequest>) -> Result<Response<pb::AllocationAccessGrant>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		let granted_by = caller_id(&request)?;
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		// The console names investors by their concierge id; resolve the way every admin
		// RPC does, so a grant lands on the money-plane row the subscribe gate reads.
		let user = resolve_target_user(&self.state, &req.user_id).await?;
		let level = parse_access(&req.level)?;
		let grant = allocations_app::grant_access(self.state.allocations.as_ref(), &service, user, level, granted_by)
			.await
			.map_err(map_err)?;
		Ok(Response::new(grant_to_proto(&grant)))
	}

	async fn revoke_allocation_access(&self, request: Request<pb::RevokeAllocationAccessRequest>) -> Result<Response<pb::RevokeAllocationAccessResponse>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		let revoked_by = caller_id(&request)?;
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		let user = resolve_target_user(&self.state, &req.user_id).await?;
		allocations_app::revoke_access(self.state.allocations.as_ref(), &service, user, revoked_by)
			.await
			.map_err(map_err)?;
		Ok(Response::new(pb::RevokeAllocationAccessResponse {}))
	}

	async fn list_allocation_access_grants(&self, request: Request<pb::ListAllocationAccessGrantsRequest>) -> Result<Response<pb::AllocationAccessGrantList>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		let service = ServiceId::parse(&request.get_ref().service).map_err(map_err)?;
		let grants = allocations_app::list_grants(self.state.allocations.as_ref(), &service).await.map_err(map_err)?;
		Ok(Response::new(pb::AllocationAccessGrantList {
			grants: grants.iter().map(grant_to_proto).collect(),
		}))
	}
}

fn record_to_proto(record: &AllocationRecord) -> pb::Allocation {
	let allocation = &record.allocation;
	pb::Allocation {
		service: allocation.service().to_string(),
		title: allocation.title().to_owned(),
		summary: allocation.summary().to_owned(),
		state: allocation.state().as_str().to_owned(),
		created_at: record.created_at,
		updated_at: record.updated_at,
		unit_cap: allocation.unit_cap().to_decimal_string(),
		icon: allocation.icon().as_str().to_owned(),
		access: allocation.access().as_str().to_owned(),
		caller_access: record.caller_access.as_str().to_owned(),
	}
}

fn grant_to_proto(grant: &AllocationAccessGrant) -> pb::AllocationAccessGrant {
	pb::AllocationAccessGrant {
		service: grant.service.to_string(),
		user_id: grant.user_id.to_string(),
		level: grant.level.as_str().to_owned(),
		granted_by: grant.granted_by.to_string(),
		granted_at: grant.granted_at,
	}
}

/// An access level a request named, parsed strictly. Unlike the icon there is no
/// "chose nothing" here: an empty level is as much a client bug as an unknown one,
/// because every level gates money and none is a safe guess. The message names the
/// vocabulary so an operator sees what was expected, not just that it was wrong. The
/// grant handler runs the same parser — `hidden` is a real level, and refusing it FOR A
/// GRANT is the aggregate's rule, so it is refused there with its own reason.
fn parse_access(raw: &str) -> Result<AllocationAccess, Status> {
	AllocationAccess::parse(raw).map_err(|_| Status::invalid_argument(format!("access level must be one of {}, got '{raw}'", wire_access::ALL.join(", "))))
}

/// The icon a *request* named. Empty means "the operator chose nothing", which is
/// [`AllocationIcon::default`]. Anything else is parsed strictly: an icon this build
/// cannot draw is an `invalid_argument` about the request, never a product silently
/// stored as something the operator did not pick.
///
/// Strict here and lenient in the storage adapter is deliberate, and the direction is
/// what decides it: client → hub rejects a value it does not know, because the request
/// has not been acted on yet and the caller can be told. Hub ← storage does not, because
/// the row is already written and refusing a read would fail `find`/`list` — the money
/// plane's gate — over a presentation column. See
/// [`crate::infrastructure::allocations`].
fn parse_icon(raw: &str) -> Result<AllocationIcon, Status> {
	if raw.is_empty() {
		return Ok(AllocationIcon::default());
	}
	AllocationIcon::parse(raw).map_err(map_err)
}

/// The same, for `UpdateAllocationRequest`, where the field carries presence.
///
/// `None` (field unset) is "the caller did not talk about the icon" and leaves the
/// stored pick alone. `Some("")` is an explicit reset to the default — a distinction a
/// bare proto3 `string` cannot express, which is why the field is `optional`: without it
/// a consumer built before the icon existed, a stale browser bundle, or a pod still on
/// the previous release would erase the operator's choice on every title edit.
fn parse_icon_update(raw: Option<&str>) -> Result<Option<AllocationIcon>, Status> {
	raw.map(parse_icon).transpose()
}

/// The wire vocabulary in `evbanking_contracts` is what consumer repos match on; the
/// domain enum is what the hub stores. They are two halves of one contract, so drift
/// between them is a compile-and-test-time failure, not a runtime mystery.
#[cfg(test)]
mod tests {
	use domain::allocations::{AllocationState, DEFAULT_UNIT_CAP};
	// Only the guard below reads the icon vocabulary — the handlers go through
	// `AllocationIcon`, which is the authority on what a stored icon may be.
	use evbanking_contracts::allocation::icon as wire_icon;

	use super::*;

	#[test]
	fn domain_states_match_the_wire_contract() {
		assert_eq!(AllocationState::Draft.as_str(), wire_state::DRAFT);
		assert_eq!(AllocationState::Open.as_str(), wire_state::OPEN);
		assert_eq!(AllocationState::Closed.as_str(), wire_state::CLOSED);
		for state in wire_state::ALL {
			assert_eq!(AllocationState::parse(state).unwrap().as_str(), state);
		}
	}

	#[test]
	fn domain_access_levels_match_the_wire_contract() {
		// The second axis, held to the same standard as state: the wire list is what
		// consumer repos match on, the domain enum is what the hub stores and RANKS, so
		// the two must agree member for member AND in order — the wire's `ALL` is
		// documented lowest-first, and a client that trusts that documentation would
		// otherwise rank a product wrong.
		let domain = [AllocationAccess::Hidden, AllocationAccess::View, AllocationAccess::Invest];
		let as_wire: Vec<&str> = domain.iter().map(|level| level.as_str()).collect();
		assert_eq!(as_wire.as_slice(), wire_access::ALL.as_slice(), "the domain enum and the wire vocabulary have drifted");
		for level in wire_access::ALL {
			assert_eq!(AllocationAccess::parse(level).unwrap().as_str(), level);
		}
		assert!(domain.windows(2).all(|pair| pair[0] < pair[1]), "the wire order must be the domain's rank order");
		assert_eq!(AllocationAccess::DEFAULT.as_str(), wire_access::DEFAULT);
		// The grantable subset is exactly what `Allocation::grant_access` admits.
		for level in wire_access::ALL {
			assert_eq!(AllocationAccess::parse(level).unwrap().permits_viewing(), wire_access::is_grantable(level), "{level}");
		}
		assert_eq!(parse_access(wire_access::INVEST).unwrap(), AllocationAccess::Invest);
		assert_eq!(parse_access("").unwrap_err().code(), tonic::Code::InvalidArgument, "no level is a safe guess");
		assert_eq!(parse_access("public").unwrap_err().code(), tonic::Code::InvalidArgument);
	}

	#[test]
	fn domain_icons_match_the_wire_contract() {
		// Same guard as the states above, for the second vocabulary: the client picks its
		// SVG off `wire_icon`, the hub stores the domain enum, and the DB CHECK in
		// migration 0027 spells the same strings. Drift here is a product that renders as
		// the wrong picture, or a row the CHECK refuses.
		//
		// Compared BOTH ways, and in order. The previous version only walked `wire_icon`
		// and parsed each entry, so a variant added to the domain (and to the CHECK, and
		// to the client) but forgotten in `wire_icon` left this green — the direction that
		// matters most, since `wire_icon` is what consumer repos match on.
		let domain: Vec<&str> = AllocationIcon::ALL.iter().map(|icon| icon.as_str()).collect();
		assert_eq!(domain.as_slice(), wire_icon::ALL.as_slice(), "the domain enum and the wire vocabulary have drifted");
		for icon in wire_icon::ALL {
			assert_eq!(AllocationIcon::parse(icon).unwrap().as_str(), icon);
		}
		for icon in AllocationIcon::ALL {
			assert!(wire_icon::is_known(icon.as_str()), "{icon:?} is a domain variant the wire contract does not name");
		}
		assert_eq!(AllocationIcon::default().as_str(), wire_icon::DEFAULT);
		// The empty wire value is "unset", not an icon — the boundary turns it into the
		// default, and everything else has to be a value the contract names.
		assert_eq!(parse_icon("").unwrap(), AllocationIcon::default());
		assert_eq!(parse_icon(wire_icon::REAL_ESTATE).unwrap(), AllocationIcon::RealEstate);
		assert_eq!(parse_icon("rocket").unwrap_err().code(), tonic::Code::InvalidArgument);
	}

	#[test]
	fn an_update_request_without_the_icon_field_asks_for_no_icon_change() {
		// The three states the `optional` field can arrive in. Absent must reach the
		// aggregate as `None` — anything else and an older consumer, a stale bundle or a
		// pod mid-rolling-deploy resets the operator's pick on every title edit.
		assert_eq!(parse_icon_update(None).unwrap(), None);
		assert_eq!(
			parse_icon_update(Some("")).unwrap(),
			Some(AllocationIcon::default()),
			"an explicit empty string is a reset, not a no-op"
		);
		assert_eq!(parse_icon_update(Some(wire_icon::VENTURE)).unwrap(), Some(AllocationIcon::Venture));
		// Strictness is unchanged for a value the caller did send.
		assert_eq!(parse_icon_update(Some("rocket")).unwrap_err().code(), tonic::Code::InvalidArgument);
	}

	#[test]
	fn the_wire_default_cap_matches_the_domain() {
		// A consumer repo renders `contracts::allocation::DEFAULT_UNIT_CAP` before an
		// operator has sized a product; it has to be the number the hub actually stored.
		assert_eq!(DEFAULT_UNIT_CAP.to_decimal_string(), evbanking_contracts::allocation::DEFAULT_UNIT_CAP);
	}
}

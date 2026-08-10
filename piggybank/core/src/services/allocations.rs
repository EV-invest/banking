//! `allocations` context — the registry of investable products.
//!
//! Reads are open to any authenticated user; every write, and the unlisted half of the
//! catalog, is gated on [`Permission::AllocationManage`] (Admin/Owner) — the same trust
//! seam as posting a valuation, because registering a product is what brings a fund into
//! existence at all.
//!
//! No money crosses this surface: the handlers below never touch the ledger or notify
//! the relay.
//!
//! `Result<_, Status>` is tonic's mandated handler signature; `Status` is a large
//! type we don't control, so the large-err lint does not apply in this module.
#![allow(clippy::result_large_err)]

use domain::{allocations::Allocation, authz::Permission, balance::ServiceId, money::Shares};
use evbanking_contracts::{
	allocation::state as wire_state,
	banking::v1::{self as pb, allocations_service_server::AllocationsService},
};
use tonic::{Request, Response, Status};

use crate::{
	AppState,
	application::allocations as allocations_app,
	ports::allocations::AllocationRecord,
	services::support::{caller_id, map_err, require_permission},
};

#[derive(Clone)]
pub struct AllocationsSvc {
	pub state: AppState,
}

impl AllocationsSvc {
	pub fn new(state: AppState) -> Self {
		Self { state }
	}
}

#[tonic::async_trait]
impl AllocationsService for AllocationsSvc {
	async fn list_allocations(&self, request: Request<pb::ListAllocationsRequest>) -> Result<Response<pb::AllocationList>, Status> {
		caller_id(&request)?;
		let include_unlisted = request.get_ref().include_unlisted;
		// Refuse rather than silently downgrade to the open-only list: a caller that asked
		// for drafts and got a filtered list would read it as "there are none".
		if include_unlisted {
			require_permission(&self.state, &request, Permission::AllocationManage).await?;
		}
		let records = allocations_app::list(self.state.allocations.as_ref(), include_unlisted).await.map_err(map_err)?;
		Ok(Response::new(pb::AllocationList {
			allocations: records.iter().map(record_to_proto).collect(),
		}))
	}

	async fn get_allocation(&self, request: Request<pb::GetAllocationRequest>) -> Result<Response<pb::Allocation>, Status> {
		// Any authenticated user, any state — an investor holding units of a closed
		// product still has to render it.
		caller_id(&request)?;
		let service = ServiceId::parse(&request.get_ref().service).map_err(map_err)?;
		let allocation = allocations_app::get(self.state.allocations.as_ref(), &service).await.map_err(map_err)?;
		Ok(Response::new(allocation_to_proto(&allocation, 0, 0)))
	}

	async fn register_allocation(&self, request: Request<pb::RegisterAllocationRequest>) -> Result<Response<pb::Allocation>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		let allocation = allocations_app::register(self.state.allocations.as_ref(), service, &req.title, &req.summary)
			.await
			.map_err(map_err)?;
		Ok(Response::new(allocation_to_proto(&allocation, 0, 0)))
	}

	async fn update_allocation(&self, request: Request<pb::UpdateAllocationRequest>) -> Result<Response<pb::Allocation>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		let allocation = allocations_app::update_details(self.state.allocations.as_ref(), &service, &req.title, &req.summary)
			.await
			.map_err(map_err)?;
		Ok(Response::new(allocation_to_proto(&allocation, 0, 0)))
	}

	async fn set_allocation_unit_cap(&self, request: Request<pb::SetAllocationUnitCapRequest>) -> Result<Response<pb::Allocation>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		// Parsed at the boundary, so a malformed cap is an `invalid_argument` about the
		// input rather than a validation error from inside the aggregate.
		let unit_cap = Shares::parse_decimal(&req.unit_cap).map_err(map_err)?;
		let allocation = allocations_app::set_unit_cap(self.state.allocations.as_ref(), &service, unit_cap).await.map_err(map_err)?;
		Ok(Response::new(allocation_to_proto(&allocation, 0, 0)))
	}

	async fn set_allocation_state(&self, request: Request<pb::SetAllocationStateRequest>) -> Result<Response<pb::Allocation>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		let allocation = match req.state.as_str() {
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
		Ok(Response::new(allocation_to_proto(&allocation, 0, 0)))
	}
}

/// A write handler returns the aggregate, which is deliberately clock-free — the
/// timestamps come back on the next read. Zero is the wire's "unset" for both, matching
/// `Position.nav_as_of`.
fn allocation_to_proto(allocation: &Allocation, created_at: i64, updated_at: i64) -> pb::Allocation {
	pb::Allocation {
		service: allocation.service().to_string(),
		title: allocation.title().to_owned(),
		summary: allocation.summary().to_owned(),
		state: allocation.state().as_str().to_owned(),
		created_at,
		updated_at,
		unit_cap: allocation.unit_cap().to_decimal_string(),
	}
}

fn record_to_proto(record: &AllocationRecord) -> pb::Allocation {
	allocation_to_proto(&record.allocation, record.created_at, record.updated_at)
}

/// The wire vocabulary in `evbanking_contracts` is what consumer repos match on; the
/// domain enum is what the hub stores. They are two halves of one contract, so drift
/// between them is a compile-and-test-time failure, not a runtime mystery.
#[cfg(test)]
mod tests {
	use domain::allocations::{AllocationState, DEFAULT_UNIT_CAP};

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
	fn the_wire_default_cap_matches_the_domain() {
		// A consumer repo renders `contracts::allocation::DEFAULT_UNIT_CAP` before an
		// operator has sized a product; it has to be the number the hub actually stored.
		assert_eq!(DEFAULT_UNIT_CAP.to_decimal_string(), evbanking_contracts::allocation::DEFAULT_UNIT_CAP);
	}
}

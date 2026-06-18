//! Context service implementations.
//!
//! One tonic service per bounded context, each holding [`AppState`] so it can
//! reach the control plane (Postgres) and ledger (TigerBeetle) as RPCs land.
//!
//! Scaffold: the proto services are empty (namespace locked, no RPCs yet), so
//! these impls are empty too. They are registered in [`super::serve`] to reserve
//! the surface; methods land here alongside the proto RPCs, per feature.

use evfund_contracts::fund::v1::{allocations_service_server::AllocationsService, balance_service_server::BalanceService, users_service_server::UsersService};

use crate::AppState;

macro_rules! context_service {
	($(#[$doc:meta])* $name:ident impl $trait:ident) => {
		$(#[$doc])*
		#[derive(Clone)]
		pub struct $name {
			pub state: AppState,
		}

		impl $name {
			pub fn new(state: AppState) -> Self {
				Self { state }
			}
		}

		impl $trait for $name {}
	};
}

context_service!(
	/// `users` context — investor account/investment RPCs land here.
	UsersSvc impl UsersService
);
context_service!(
	/// `balance` context — company-money RPCs land here.
	BalanceSvc impl BalanceService
);
context_service!(
	/// `allocations` context — capital-allocation RPCs land here.
	AllocationsSvc impl AllocationsService
);

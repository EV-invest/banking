//! Context service implementations — one tonic service per bounded context.
//!
//! Every RPC is authorized from the verified [`Claims`](evbanking_auth::Claims)
//! injected by core's inbound auth layer. Self-service RPCs act on the caller's own
//! `sub` ([`caller_id`]); admin RPCs are gated by the hub's allowlist
//! ([`require_admin`]). The stateful money rules (e.g. who may revoke an allocation)
//! live in the aggregate, applied under a row lock — the boundary here only does the
//! cheap "are you this principal?" check (defense in depth).
//!
//! `Result<_, Status>` is tonic's mandated handler signature; `Status` is a large
//! type we don't control, so the large-err lint does not apply in this module.
#![allow(clippy::result_large_err)]

use std::time::{SystemTime, UNIX_EPOCH};

use domain::{
	allocations::{Allocation, AllocationId, AllocationKind},
	balance::{LedgerAccountKey, Party, ServiceId},
	error::DomainError,
	money::{Network, TxRef, Usdt},
	users::UserId,
};
use evbanking_auth::claims_of;
use evbanking_contracts::banking::v1::{self as pb, allocations_service_server::AllocationsService, balance_service_server::BalanceService, users_service_server::UsersService};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{
	AppState,
	application::{allocations as alloc_app, balance as balance_app},
};

/// `users` context — investor account/investment RPCs.
#[derive(Clone)]
pub struct UsersSvc {
	pub state: AppState,
}

impl UsersSvc {
	pub fn new(state: AppState) -> Self {
		Self { state }
	}
}

#[tonic::async_trait]
impl UsersService for UsersSvc {
	async fn get_me(&self, request: Request<pb::GetMeRequest>) -> Result<Response<pb::UserProfile>, Status> {
		let id = caller_id(&request)?;
		let user = self.state.users.find_by_id(id).await.map_err(map_err)?.ok_or_else(|| Status::not_found("user"))?;
		Ok(Response::new(pb::UserProfile {
			user_id: user.id().to_string(),
			email: user.email().as_str().to_owned(),
			email_verified: user.email_verified(),
			status: user.status().as_str().to_owned(),
			token_version: user.token_version(),
		}))
	}

	async fn get_balance(&self, request: Request<pb::GetUserBalanceRequest>) -> Result<Response<pb::UserBalanceResponse>, Status> {
		let id = caller_id(&request)?;
		// The user's balance is the sum of their per-network claims, read live from
		// TigerBeetle (Read-First). Each claim is credit-normal, so non-negative.
		let mut amount = Usdt::ZERO;
		let mut pending = Usdt::ZERO;
		for network in Network::ALL {
			let balance = self
				.state
				.ledger
				.balance(&LedgerAccountKey::UserClaim(id, network))
				.await
				.map_err(|_| Status::unavailable("ledger unavailable"))?;
			amount = amount.checked_add(balance.posted).ok_or_else(|| Status::internal("balance overflow"))?;
			pending = pending.checked_add(balance.pending).ok_or_else(|| Status::internal("balance overflow"))?;
		}
		Ok(Response::new(pb::UserBalanceResponse {
			amount: amount.to_decimal_string(),
			pending: pending.to_decimal_string(),
			authoritative: true,
			as_of: unix_now(),
		}))
	}

	async fn revoke_tokens(&self, request: Request<pb::RevokeTokensRequest>) -> Result<Response<pb::RevokeTokensResponse>, Status> {
		require_admin(&self.state, &request)?;
		let target = parse_user_id(&request.get_ref().user_id)?;
		let mut user = self.state.users.find_by_id(target).await.map_err(map_err)?.ok_or_else(|| Status::not_found("user"))?;
		let token_version = user.revoke_tokens();
		self.state.users.save(&mut user).await.map_err(map_err)?;
		Ok(Response::new(pb::RevokeTokensResponse { token_version }))
	}

	async fn disable_user(&self, request: Request<pb::DisableUserRequest>) -> Result<Response<pb::DisableUserResponse>, Status> {
		require_admin(&self.state, &request)?;
		let target = parse_user_id(&request.get_ref().user_id)?;
		let mut user = self.state.users.find_by_id(target).await.map_err(map_err)?.ok_or_else(|| Status::not_found("user"))?;
		user.disable();
		self.state.users.save(&mut user).await.map_err(map_err)?;
		Ok(Response::new(pb::DisableUserResponse {}))
	}
}

/// `balance` context — company-money RPCs (all admin-gated).
#[derive(Clone)]
pub struct BalanceSvc {
	pub state: AppState,
}

impl BalanceSvc {
	pub fn new(state: AppState) -> Self {
		Self { state }
	}
}

#[tonic::async_trait]
impl BalanceService for BalanceSvc {
	async fn get_fund_balance(&self, request: Request<pb::GetFundBalanceRequest>) -> Result<Response<pb::FundBalance>, Status> {
		require_admin(&self.state, &request)?;
		let balance = balance_app::fund_balance(self.state.ledger.as_ref()).await.map_err(map_err)?;
		Ok(Response::new(pb::FundBalance {
			networks: balance
				.networks
				.into_iter()
				.map(|n| pb::NetworkBalance {
					network: n.network.as_str().to_owned(),
					custody: n.custody.to_decimal_string(),
					fund_free: n.fund_free.to_decimal_string(),
					allocated: n.allocated.to_decimal_string(),
				})
				.collect(),
		}))
	}

	async fn seed_capital(&self, request: Request<pb::SeedCapitalRequest>) -> Result<Response<pb::SeedCapitalResponse>, Status> {
		require_admin(&self.state, &request)?;
		let req = request.into_inner();
		let network = Network::parse(&req.network).map_err(map_err)?;
		let amount = Usdt::parse_decimal(&req.amount).map_err(map_err)?;
		balance_app::seed_fund_capital(&self.state.pool, &self.state.relay_notify, network, amount)
			.await
			.map_err(map_err)?;
		Ok(Response::new(pb::SeedCapitalResponse {}))
	}

	async fn record_deposit(&self, request: Request<pb::RecordDepositRequest>) -> Result<Response<pb::RecordDepositResponse>, Status> {
		require_admin(&self.state, &request)?;
		let req = request.into_inner();
		let tx_ref = TxRef::parse(&req.tx_ref).map_err(map_err)?;
		let network = Network::parse(&req.network).map_err(map_err)?;
		let amount = Usdt::parse_decimal(&req.amount).map_err(map_err)?;
		let party = Party::from_parts(&req.party_kind, optional(&req.party_id)).map_err(map_err)?;
		let recorded = balance_app::record_deposit(&self.state.pool, &self.state.relay_notify, tx_ref, party, network, amount)
			.await
			.map_err(map_err)?;
		Ok(Response::new(pb::RecordDepositResponse { recorded }))
	}
}

/// `allocations` context — capital-allocation RPCs. The user-facing surface acts on
/// the caller's own `sub`; service-side reservation/settlement RPCs land in a follow-up.
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
	async fn allocate(&self, request: Request<pb::AllocateRequest>) -> Result<Response<pb::Allocation>, Status> {
		let user = caller_id(&request)?;
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		let network = Network::parse(&req.network).map_err(map_err)?;
		let amount = Usdt::parse_decimal(&req.amount).map_err(map_err)?;
		let allocation = alloc_app::allocate_user_stake(
			self.state.allocations.as_ref(),
			self.state.ledger.as_ref(),
			&self.state.relay_notify,
			user,
			service,
			network,
			amount,
		)
		.await
		.map_err(map_err)?;
		Ok(Response::new(allocation_to_proto(&allocation)))
	}

	async fn revoke_allocation(&self, request: Request<pb::RevokeAllocationRequest>) -> Result<Response<pb::Allocation>, Status> {
		let user = caller_id(&request)?;
		let id = parse_allocation_id(&request.get_ref().allocation_id)?;
		let allocation = alloc_app::revoke_user_stake(self.state.allocations.as_ref(), &self.state.relay_notify, id, user)
			.await
			.map_err(map_err)?;
		Ok(Response::new(allocation_to_proto(&allocation)))
	}

	async fn list_allocations(&self, request: Request<pb::ListAllocationsRequest>) -> Result<Response<pb::AllocationList>, Status> {
		let user = caller_id(&request)?;
		let allocations = alloc_app::list_user_allocations(self.state.allocations.as_ref(), user).await.map_err(map_err)?;
		Ok(Response::new(pb::AllocationList {
			allocations: allocations.iter().map(allocation_to_proto).collect(),
		}))
	}
}

/// The authenticated caller's own user id (from the access-token `sub`).
fn caller_id<T>(request: &Request<T>) -> Result<UserId, Status> {
	let claims = claims_of(request).ok_or_else(|| Status::unauthenticated("missing claims"))?;
	parse_user_id(&claims.sub)
}

/// Gate an RPC on the admin allowlist. Only a human access token can be an admin —
/// a service token (distinct `typ`) never qualifies, even if its `sub` matched.
fn require_admin<T>(state: &AppState, request: &Request<T>) -> Result<(), Status> {
	let claims = claims_of(request).ok_or_else(|| Status::unauthenticated("missing claims"))?;
	if claims.is_access() && state.is_admin(&claims.sub) {
		Ok(())
	} else {
		Err(Status::permission_denied("admin only"))
	}
}

fn parse_user_id(raw: &str) -> Result<UserId, Status> {
	Uuid::parse_str(raw).map(UserId::from_raw).map_err(|_| Status::unauthenticated("subject is not a user id"))
}

fn parse_allocation_id(raw: &str) -> Result<AllocationId, Status> {
	Uuid::parse_str(raw).map(AllocationId::from_raw).map_err(|_| Status::invalid_argument("invalid allocation id"))
}

fn allocation_to_proto(allocation: &Allocation) -> pb::Allocation {
	let (kind, service) = match allocation.kind() {
		AllocationKind::UserStake { service, .. } => ("user_stake", service.as_str().to_owned()),
		AllocationKind::ServiceReservation { service } => ("service_reservation", service.as_str().to_owned()),
		AllocationKind::ServiceHolding { service } => ("service_holding", service.as_str().to_owned()),
	};
	pb::Allocation {
		id: allocation.id().to_string(),
		amount: allocation.amount().to_decimal_string(),
		network: allocation.network().as_str().to_owned(),
		owner_kind: allocation.owner().kind_str().to_owned(),
		owner_id: allocation.owner().id_str().unwrap_or_default(),
		sharers: allocation
			.sharers()
			.iter()
			.map(|party| pb::Sharer {
				kind: party.kind_str().to_owned(),
				id: party.id_str().unwrap_or_default(),
			})
			.collect(),
		kind: kind.to_owned(),
		service,
		state: allocation.state().as_str().to_owned(),
	}
}

/// Treat an empty proto string field as an absent optional.
fn optional(raw: &str) -> Option<&str> {
	if raw.is_empty() { None } else { Some(raw) }
}

fn unix_now() -> i64 {
	SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or_default()
}

/// Map a domain error to a gRPC status without leaking control-plane internals.
fn map_err(err: DomainError) -> Status {
	match err {
		DomainError::NotFound { .. } => Status::not_found(err.to_string()),
		DomainError::Validation(_) => Status::invalid_argument(err.to_string()),
		DomainError::Forbidden(_) => Status::permission_denied(err.to_string()),
		DomainError::Conflict(_) => Status::already_exists(err.to_string()),
		DomainError::Repository(_) => Status::unavailable("internal error"),
	}
}

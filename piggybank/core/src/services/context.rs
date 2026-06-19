//! Context service implementations — one tonic service per bounded context.
//!
//! [`UsersSvc`] is the first real surface: every RPC is authorized from the
//! verified [`Claims`](evbanking_auth::Claims) injected by core's inbound auth
//! layer. Self-service RPCs act on the caller's own `sub`; admin RPCs are gated by
//! the hub's admin allowlist (config). `balance`/`allocations` stay empty (their
//! proto services have no RPCs yet), registered to reserve the surface.
//!
//! `Result<_, Status>` is tonic's mandated handler signature, and the helpers here
//! feed it; `Status` is a large type we don't control, so the large-err lint does
//! not apply in this module.
#![allow(clippy::result_large_err)]

use std::time::{SystemTime, UNIX_EPOCH};

use domain::{error::DomainError, users::UserId};
use evbanking_auth::claims_of;
use evbanking_contracts::banking::v1::{
	DisableUserRequest, DisableUserResponse, GetMeRequest, GetUserBalanceRequest, RevokeTokensRequest, RevokeTokensResponse, UserBalanceResponse, UserProfile,
	allocations_service_server::AllocationsService, balance_service_server::BalanceService, users_service_server::UsersService,
};
use tonic::{Request, Response, Status};
use uuid::Uuid;

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
	/// `balance` context — company-money RPCs land here.
	BalanceSvc impl BalanceService
);
context_service!(
	/// `allocations` context — capital-allocation RPCs land here.
	AllocationsSvc impl AllocationsService
);

/// `users` context — investor account/investment RPCs.
#[derive(Clone)]
pub struct UsersSvc {
	pub state: AppState,
}

impl UsersSvc {
	pub fn new(state: AppState) -> Self {
		Self { state }
	}

	fn require_admin<T>(&self, request: &Request<T>) -> Result<(), Status> {
		let claims = claims_of(request).ok_or_else(|| Status::unauthenticated("missing claims"))?;
		// Only a human access token can be an admin — a service token (distinct `typ`)
		// never satisfies the allowlist, even if its `sub` somehow matched.
		if claims.is_access() && self.state.is_admin(&claims.sub) {
			Ok(())
		} else {
			Err(Status::permission_denied("admin only"))
		}
	}
}

#[tonic::async_trait]
impl UsersService for UsersSvc {
	async fn get_me(&self, request: Request<GetMeRequest>) -> Result<Response<UserProfile>, Status> {
		let id = caller_id(&request)?;
		let user = self.state.users.find_by_id(id).await.map_err(map_err)?.ok_or_else(|| Status::not_found("user"))?;
		Ok(Response::new(UserProfile {
			user_id: user.id().to_string(),
			email: user.email().as_str().to_owned(),
			email_verified: user.email_verified(),
			status: user.status().as_str().to_owned(),
			token_version: user.token_version(),
		}))
	}

	async fn get_balance(&self, request: Request<GetUserBalanceRequest>) -> Result<Response<UserBalanceResponse>, Status> {
		let id = caller_id(&request)?;

		// Resolve the UUID↔u128 id-map (control plane); never reads an amount.
		let account_id = sqlx::query_scalar::<_, Vec<u8>>("SELECT tb_account_id FROM ledger_accounts WHERE user_id = $1")
			.bind(id.raw())
			.fetch_optional(&self.state.pool)
			.await
			.map_err(|_| Status::unavailable("control plane unavailable"))?;

		// Read the live balance from TigerBeetle (Read-First). No id-map row ⇒ the
		// user has no ledger account yet, so the balance is definitionally zero.
		let (amount, pending) = match account_id {
			None => (0u128, 0u128),
			Some(bytes) => {
				let tb_id = u128_from_be(&bytes)?;
				let accounts = self
					.state
					.tigerbeetle
					.client()
					.lookup_accounts(&[tb_id])
					.map_err(|_| Status::unavailable("ledger unavailable"))?
					.await
					.map_err(|_| Status::unavailable("ledger unavailable"))?;
				match accounts.first() {
					// An investor account is credit-normal, so credits ≥ debits by
					// invariant; a checked underflow surfaces a ledger inconsistency
					// rather than silently reporting a wrong (clamped) balance.
					Some(account) => {
						let amount = account
							.credits_posted
							.checked_sub(account.debits_posted)
							.ok_or_else(|| Status::internal("ledger balance underflow"))?;
						let pending = account
							.credits_pending
							.checked_sub(account.debits_pending)
							.ok_or_else(|| Status::internal("ledger balance underflow"))?;
						(amount, pending)
					}
					None => (0, 0),
				}
			}
		};

		Ok(Response::new(UserBalanceResponse {
			amount: amount.to_string(),
			pending: pending.to_string(),
			authoritative: true,
			as_of: unix_now(),
		}))
	}

	async fn revoke_tokens(&self, request: Request<RevokeTokensRequest>) -> Result<Response<RevokeTokensResponse>, Status> {
		self.require_admin(&request)?;
		let target = parse_user_id(&request.get_ref().user_id)?;
		let mut user = self.state.users.find_by_id(target).await.map_err(map_err)?.ok_or_else(|| Status::not_found("user"))?;
		let token_version = user.revoke_tokens();
		self.state.users.save(&mut user).await.map_err(map_err)?;
		Ok(Response::new(RevokeTokensResponse { token_version }))
	}

	async fn disable_user(&self, request: Request<DisableUserRequest>) -> Result<Response<DisableUserResponse>, Status> {
		self.require_admin(&request)?;
		let target = parse_user_id(&request.get_ref().user_id)?;
		let mut user = self.state.users.find_by_id(target).await.map_err(map_err)?.ok_or_else(|| Status::not_found("user"))?;
		user.disable();
		self.state.users.save(&mut user).await.map_err(map_err)?;
		Ok(Response::new(DisableUserResponse {}))
	}
}

/// The authenticated caller's own user id (from the access-token `sub`).
fn caller_id<T>(request: &Request<T>) -> Result<UserId, Status> {
	let claims = claims_of(request).ok_or_else(|| Status::unauthenticated("missing claims"))?;
	parse_user_id(&claims.sub)
}

fn parse_user_id(raw: &str) -> Result<UserId, Status> {
	Uuid::parse_str(raw).map(UserId::from_raw).map_err(|_| Status::unauthenticated("subject is not a user id"))
}

fn u128_from_be(bytes: &[u8]) -> Result<u128, Status> {
	let array: [u8; 16] = bytes.try_into().map_err(|_| Status::internal("malformed ledger account id"))?;
	Ok(u128::from_be_bytes(array))
}

fn unix_now() -> i64 {
	SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or_default()
}

/// Map a domain error to a gRPC status without leaking control-plane internals.
fn map_err(err: DomainError) -> Status {
	match err {
		DomainError::NotFound { .. } => Status::not_found(err.to_string()),
		DomainError::Validation(_) => Status::invalid_argument(err.to_string()),
		DomainError::Conflict(_) => Status::already_exists(err.to_string()),
		DomainError::Repository(_) => Status::unavailable("internal error"),
	}
}

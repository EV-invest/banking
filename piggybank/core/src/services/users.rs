//! `users` context — investor account/investment RPCs.
//!
//! Every RPC is authorized from the verified [`Claims`](evbanking_auth::Claims)
//! injected by core's inbound auth layer. Self-service RPCs act on the caller's own
//! `sub` (`caller_id`); admin RPCs are gated by the hub's allowlist
//! (`require_admin`).
//!
//! `Result<_, Status>` is tonic's mandated handler signature; `Status` is a large
//! type we don't control, so the large-err lint does not apply in this module.
#![allow(clippy::result_large_err)]

use domain::{
	authz::Permission,
	balance::LedgerAccountKey,
	money::Usdt,
	users::{ConciergeUserId, ProfileFields, User, UserId},
};
use evbanking_contracts::banking::v1::{self as pb, users_service_server::UsersService};
use tonic::{Request, Response, Status};

use crate::{
	AppState,
	application::users as users_app,
	services::support::{caller_id, map_err, optional, parse_user_id, require_permission, unix_now},
};

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
		Ok(Response::new(user_to_proto(&user)))
	}

	async fn update_profile(&self, request: Request<pb::UpdateProfileRequest>) -> Result<Response<pb::UserProfile>, Status> {
		let id = caller_id(&request)?;
		let req = request.into_inner();
		let fields = ProfileFields {
			legal_name: optional(&req.legal_name).map(str::to_owned),
			preferred_name: optional(&req.preferred_name).map(str::to_owned),
			phone: optional(&req.phone).map(str::to_owned),
			date_of_birth: optional(&req.date_of_birth).map(str::to_owned),
			nationality: optional(&req.nationality).map(str::to_owned),
			tax_residence: optional(&req.tax_residence).map(str::to_owned),
			residential_address: optional(&req.residential_address).map(str::to_owned),
			language: optional(&req.language).map(str::to_owned),
			base_currency: optional(&req.base_currency).map(str::to_owned),
			timezone: optional(&req.timezone).map(str::to_owned),
		};
		let user = users_app::update_profile(self.state.users.as_ref(), id, fields).await.map_err(map_err)?;
		Ok(Response::new(user_to_proto(&user)))
	}

	async fn get_balance(&self, request: Request<pb::GetUserBalanceRequest>) -> Result<Response<pb::UserBalanceResponse>, Status> {
		let id = caller_id(&request)?;
		// The user's single unified claim, read live from TigerBeetle (Read-First);
		// credit-normal, so non-negative.
		let balance = self
			.state
			.ledger
			.balance(&LedgerAccountKey::UserClaim(id))
			.await
			.map_err(|_| Status::unavailable("ledger unavailable"))?;
		Ok(Response::new(pb::UserBalanceResponse {
			amount: Usdt::from_base_units(balance.posted).to_decimal_string(),
			pending: Usdt::from_base_units(balance.pending).to_decimal_string(),
			authoritative: true,
			as_of: unix_now(),
		}))
	}

	async fn revoke_tokens(&self, request: Request<pb::RevokeTokensRequest>) -> Result<Response<pb::RevokeTokensResponse>, Status> {
		require_permission(&self.state, &request, Permission::UserRevoke).await?;
		let target = parse_user_id(&request.get_ref().user_id)?;
		let user = users_app::revoke_tokens(self.state.users.as_ref(), target).await.map_err(map_err)?;
		Ok(Response::new(pb::RevokeTokensResponse {
			token_version: user.token_version(),
		}))
	}

	async fn disable_user(&self, request: Request<pb::DisableUserRequest>) -> Result<Response<pb::DisableUserResponse>, Status> {
		require_permission(&self.state, &request, Permission::UserSuspend).await?;
		let target = parse_user_id(&request.get_ref().user_id)?;
		users_app::disable_user(self.state.users.as_ref(), target).await.map_err(map_err)?;
		Ok(Response::new(pb::DisableUserResponse {}))
	}

	async fn get_user_balance(&self, request: Request<pb::AdminBalanceRequest>) -> Result<Response<pb::UserBalanceResponse>, Status> {
		require_permission(&self.state, &request, Permission::UserBalanceRead).await?;
		let raw = parse_user_id(&request.get_ref().user_id)?.raw();
		// The operator console carries CONCIERGE ids (the identity plane's ListUsers) while
		// money-plane callers (the redemption queue) carry banking ids — resolve concierge-
		// first via the bridge mirror, then fall back to the banking id; an id matching
		// neither is NOT_FOUND, never an authoritative zero. `disabled`/`token_version` on
		// the target are deliberately ignored: an operator must be able to inspect a
		// frozen/disabled user's balance.
		let target = match self.state.users.resolve_issuance_by_concierge_id(ConciergeUserId::from_raw(raw)).await.map_err(map_err)? {
			Some(target) => target.user_id,
			None =>
				self.state
					.users
					.resolve_issuance_by_banking_id(UserId::from_raw(raw))
					.await
					.map_err(map_err)?
					.ok_or_else(|| Status::not_found("user"))?
					.user_id,
		};
		// The user's single unified claim, read live from TigerBeetle (Read-First).
		let balance = self
			.state
			.ledger
			.balance(&LedgerAccountKey::UserClaim(target))
			.await
			.map_err(|_| Status::unavailable("ledger unavailable"))?;
		Ok(Response::new(pb::UserBalanceResponse {
			amount: Usdt::from_base_units(balance.posted).to_decimal_string(),
			pending: Usdt::from_base_units(balance.pending).to_decimal_string(),
			authoritative: true,
			as_of: unix_now(),
		}))
	}
}

fn user_to_proto(user: &User) -> pb::UserProfile {
	pb::UserProfile {
		user_id: user.id().to_string(),
		email: user.email().as_str().to_owned(),
		email_verified: user.email_verified(),
		status: user.status().as_str().to_owned(),
		token_version: user.token_version(),
		legal_name: user.legal_name().unwrap_or_default().to_owned(),
		preferred_name: user.preferred_name().unwrap_or_default().to_owned(),
		phone: user.phone().unwrap_or_default().to_owned(),
		date_of_birth: user.date_of_birth().unwrap_or_default().to_owned(),
		nationality: user.nationality().unwrap_or_default().to_owned(),
		tax_residence: user.tax_residence().unwrap_or_default().to_owned(),
		residential_address: user.residential_address().unwrap_or_default().to_owned(),
		language: user.language().unwrap_or_default().to_owned(),
		base_currency: user.base_currency().unwrap_or_default().to_owned(),
		timezone: user.timezone().unwrap_or_default().to_owned(),
		// Identity (kyc/role) is OWNED by concierge; banking mirrors it onto the bridge
		// projection for gating but does not re-serve it on this self-service profile (the
		// cabinet reads identity from concierge). Default here — present for wire parity.
		kyc_level: 0,
		role: String::new(),
	}
}

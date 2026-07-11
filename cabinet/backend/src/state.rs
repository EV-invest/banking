use std::{sync::Arc, time::Duration};

use evbanking_contracts::banking::v1 as bk;
use evconcierge_contracts::concierge::v1 as cc;
use tonic::{
	Request, Status,
	transport::{Channel, Endpoint},
};

use crate::{config::AppConfig, cookies::CookieNames, session::BankingTokens};

/// Cap on establishing a TCP/TLS connection to an upstream plane: a black-holed or
/// half-open replica must fail fast rather than wedge the awaiting request task.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// Per-RPC deadline applied to every upstream call. tonic has no default request
/// timeout, so without this a wedged plane keeps the browser-facing task (and the
/// transparent-refresh single-flight lock in [`session`](crate::session)) alive
/// indefinitely. The router's [`TimeoutLayer`](tower_http::timeout::TimeoutLayer) is a
/// slightly looser outer bound, so an upstream stall surfaces here first as an error.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Shared application state, cheaply cloneable per request.
#[derive(Clone)]
pub struct AppState {
	pub config: Arc<AppConfig>,
	pub grpc: Grpc,
	/// The per-user banking money-pair cache (the one server-side state left — auth
	/// is shell-owned and the request credential is the verified access-JWT cookie).
	pub banking: Arc<BankingTokens>,
	pub cookies: Arc<CookieNames>,
	/// Local verifier for the shared concierge access JWT (JWKS-cached — no
	/// per-request round trip).
	pub verifier: evconcierge_auth::Verifier,
}

/// gRPC egress to both planes. Channels are lazily connected and cheap to clone, so a
/// fresh typed client is built per call from the shared channel.
#[derive(Clone)]
pub struct Grpc {
	piggybank: Channel,
	banking_auth: Channel,
	concierge: Channel,
	/// The shared bearer presented on the banking `IssueUserToken` seam. `None` ⇒ no
	/// money-plane token is minted (money routes surface `NotConfigured`).
	banking_issuance_token: Option<Arc<str>>,
}
impl Grpc {
	pub fn connect_lazy(piggybank_addr: &str, banking_auth_addr: &str, concierge_addr: &str, banking_issuance_token: Option<String>) -> anyhow::Result<Self> {
		let piggybank = endpoint(piggybank_addr)?.connect_lazy();
		let banking_auth = endpoint(banking_auth_addr)?.connect_lazy();
		let concierge = endpoint(concierge_addr)?.connect_lazy();
		Ok(Self {
			piggybank,
			banking_auth,
			concierge,
			banking_issuance_token: banking_issuance_token.map(Arc::from),
		})
	}

	fn banking_auth(&self) -> bk::auth_service_client::AuthServiceClient<Channel> {
		bk::auth_service_client::AuthServiceClient::new(self.banking_auth.clone())
	}

	fn directory(&self) -> cc::user_directory_client::UserDirectoryClient<Channel> {
		cc::user_directory_client::UserDirectoryClient::new(self.concierge.clone())
	}

	fn wallet(&self) -> bk::wallet_service_client::WalletServiceClient<Channel> {
		bk::wallet_service_client::WalletServiceClient::new(self.piggybank.clone())
	}

	fn funds(&self) -> bk::funds_service_client::FundsServiceClient<Channel> {
		bk::funds_service_client::FundsServiceClient::new(self.piggybank.clone())
	}

	fn health(&self) -> bk::health_service_client::HealthServiceClient<Channel> {
		bk::health_service_client::HealthServiceClient::new(self.piggybank.clone())
	}

	fn users_svc(&self) -> bk::users_service_client::UsersServiceClient<Channel> {
		bk::users_service_client::UsersServiceClient::new(self.piggybank.clone())
	}

	fn balance(&self) -> bk::balance_service_client::BalanceServiceClient<Channel> {
		bk::balance_service_client::BalanceServiceClient::new(self.piggybank.clone())
	}

	fn platform(&self) -> cc::platform_service_client::PlatformServiceClient<Channel> {
		cc::platform_service_client::PlatformServiceClient::new(self.concierge.clone())
	}

	fn concierge_health(&self) -> cc::health_service_client::HealthServiceClient<Channel> {
		cc::health_service_client::HealthServiceClient::new(self.concierge.clone())
	}

	// ── concierge identity plane ───────────────────────────────────────────────
	// The auth issuance RPCs (Exchange/Refresh/Logout/sessions) left with the OAuth
	// flow — the shell-owned auth surface calls them; the BFF only forwards the
	// verified user token to the directory.
	pub async fn get_me(&self, token: &str) -> Result<cc::UserProfile, Status> {
		Ok(self.directory().get_me(bearer(token, cc::GetMeRequest {})?).await?.into_inner())
	}

	pub async fn update_profile(&self, token: &str, req: cc::UpdateProfileRequest) -> Result<cc::UserProfile, Status> {
		Ok(self.directory().update_profile(bearer(token, req)?).await?.into_inner())
	}

	// ── banking auth plane (money-plane token issuance) ─────────────────────────
	/// Mint the money-plane token pair for the concierge-authenticated user — the
	/// concierge→banking exchange seam. Authenticated by the shared issuance token (NOT a
	/// user token); banking maps the concierge id to its bridge-mirrored row. Errors with
	/// `UNAVAILABLE` when issuance is not configured, so the caller leaves the pair empty.
	pub async fn issue_banking_token(&self, concierge_user_id: &str, user_agent: &str, ip: &str) -> Result<bk::TokenResponse, Status> {
		let token = self.banking_issuance_token.as_deref().ok_or_else(|| Status::unavailable("banking issuance not configured"))?;
		let req = bk::IssueUserTokenRequest {
			concierge_user_id: concierge_user_id.to_string(),
			user_agent: user_agent.to_string(),
			ip: ip.to_string(),
		};
		Ok(self.banking_auth().issue_user_token(bearer(token, req)?).await?.into_inner())
	}

	/// Rotate the money-plane refresh token (the banking-side family), independent of the
	/// concierge refresh. The public credential is the refresh token, not a user token.
	pub async fn refresh_banking_token(&self, refresh_token: &str) -> Result<bk::TokenResponse, Status> {
		let req = bk::RefreshRequest {
			refresh_token: refresh_token.to_string(),
		};
		Ok(self.banking_auth().refresh(req).await?.into_inner())
	}

	// ── piggybank money plane ──────────────────────────────────────────────────
	pub async fn check(&self) -> Result<bk::CheckResponse, Status> {
		Ok(self.health().check(bk::CheckRequest {}).await?.into_inner())
	}

	pub async fn get_wallet(&self, token: &str) -> Result<bk::Wallet, Status> {
		Ok(self.wallet().get_wallet(bearer(token, bk::GetWalletRequest {})?).await?.into_inner())
	}

	pub async fn deposit_address(&self, token: &str, network: &str) -> Result<bk::DepositAddress, Status> {
		let req = bk::GetDepositAddressRequest { network: network.to_string() };
		Ok(self.wallet().get_deposit_address(bearer(token, req)?).await?.into_inner())
	}

	pub async fn request_withdrawal(&self, token: &str, req: bk::RequestWithdrawalRequest) -> Result<bk::Withdrawal, Status> {
		Ok(self.wallet().request_withdrawal(bearer(token, req)?).await?.into_inner())
	}

	pub async fn cancel_withdrawal(&self, token: &str, withdrawal_id: &str) -> Result<bk::Withdrawal, Status> {
		let req = bk::CancelWithdrawalRequest {
			withdrawal_id: withdrawal_id.to_string(),
		};
		Ok(self.wallet().cancel_withdrawal(bearer(token, req)?).await?.into_inner())
	}

	pub async fn list_withdrawals(&self, token: &str) -> Result<bk::WithdrawalList, Status> {
		Ok(self.wallet().list_withdrawals(bearer(token, bk::ListWithdrawalsRequest {})?).await?.into_inner())
	}

	pub async fn list_deposits(&self, token: &str) -> Result<bk::DepositList, Status> {
		Ok(self.wallet().list_deposits(bearer(token, bk::ListDepositsRequest {})?).await?.into_inner())
	}

	pub async fn subscribe(&self, token: &str, req: bk::SubscribeRequest) -> Result<bk::Subscription, Status> {
		Ok(self.funds().subscribe(bearer(token, req)?).await?.into_inner())
	}

	pub async fn redeem(&self, token: &str, req: bk::RedeemRequest) -> Result<bk::Redemption, Status> {
		Ok(self.funds().redeem(bearer(token, req)?).await?.into_inner())
	}

	pub async fn cancel_redemption(&self, token: &str, redemption_id: &str) -> Result<bk::Redemption, Status> {
		let req = bk::CancelRedemptionRequest {
			redemption_id: redemption_id.to_string(),
		};
		Ok(self.funds().cancel_redemption(bearer(token, req)?).await?.into_inner())
	}

	pub async fn list_positions(&self, token: &str) -> Result<bk::PositionList, Status> {
		Ok(self.funds().list_positions(bearer(token, bk::ListPositionsRequest {})?).await?.into_inner())
	}

	pub async fn list_redemptions(&self, token: &str) -> Result<bk::RedemptionList, Status> {
		Ok(self.funds().list_redemptions(bearer(token, bk::ListRedemptionsRequest {})?).await?.into_inner())
	}

	pub async fn fund_nav(&self, token: &str, service: &str) -> Result<bk::FundNav, Status> {
		let req = bk::GetFundNavRequest { service: service.to_string() };
		Ok(self.funds().get_fund_nav(bearer(token, req)?).await?.into_inner())
	}

	pub async fn readiness(&self) -> Result<bk::ReadinessResponse, Status> {
		Ok(self.health().readiness(bk::ReadinessRequest {}).await?.into_inner())
	}

	pub async fn concierge_check(&self) -> Result<cc::CheckResponse, Status> {
		Ok(self.concierge_health().check(cc::CheckRequest {}).await?.into_inner())
	}

	// ── admin: concierge identity plane (identity token) ────────────────────────
	pub async fn admin_list_users(&self, token: &str, req: cc::ListUsersRequest) -> Result<cc::ListUsersResponse, Status> {
		Ok(self.directory().list_users(bearer(token, req)?).await?.into_inner())
	}

	pub async fn admin_get_user(&self, token: &str, user_id: &str) -> Result<cc::UserProfile, Status> {
		let req = cc::GetUserRequest { user_id: user_id.to_string() };
		Ok(self.directory().get_user(bearer(token, req)?).await?.into_inner())
	}

	pub async fn admin_set_role(&self, token: &str, user_id: &str, role: &str) -> Result<cc::SetRoleResponse, Status> {
		let req = cc::SetRoleRequest {
			user_id: user_id.to_string(),
			role: role.to_string(),
		};
		Ok(self.directory().set_role(bearer(token, req)?).await?.into_inner())
	}

	pub async fn admin_disable_user(&self, token: &str, user_id: &str) -> Result<(), Status> {
		let req = cc::DisableUserRequest { user_id: user_id.to_string() };
		self.directory().disable_user(bearer(token, req)?).await?;
		Ok(())
	}

	pub async fn admin_reinstate_user(&self, token: &str, user_id: &str) -> Result<(), Status> {
		let req = cc::ReinstateUserRequest { user_id: user_id.to_string() };
		self.directory().reinstate_user(bearer(token, req)?).await?;
		Ok(())
	}

	pub async fn admin_revoke_tokens(&self, token: &str, user_id: &str) -> Result<cc::RevokeTokensResponse, Status> {
		let req = cc::RevokeTokensRequest { user_id: user_id.to_string() };
		Ok(self.directory().revoke_tokens(bearer(token, req)?).await?.into_inner())
	}

	pub async fn admin_set_kyc_level(&self, token: &str, user_id: &str, kyc_level: u32) -> Result<cc::SetKycLevelResponse, Status> {
		let req = cc::SetKycLevelRequest {
			user_id: user_id.to_string(),
			kyc_level,
		};
		Ok(self.directory().set_kyc_level(bearer(token, req)?).await?.into_inner())
	}

	pub async fn platform_config(&self, token: &str) -> Result<cc::PlatformConfig, Status> {
		Ok(self.platform().get_platform_config(bearer(token, cc::GetPlatformConfigRequest {})?).await?.into_inner())
	}

	pub async fn set_maintenance_mode(&self, token: &str, enabled: bool) -> Result<cc::PlatformConfig, Status> {
		let req = cc::SetMaintenanceModeRequest { enabled };
		Ok(self.platform().set_maintenance_mode(bearer(token, req)?).await?.into_inner())
	}

	pub async fn set_announcement(&self, token: &str, req: cc::SetAnnouncementRequest) -> Result<cc::PlatformConfig, Status> {
		Ok(self.platform().set_announcement(bearer(token, req)?).await?.into_inner())
	}

	pub async fn set_feature_flag(&self, token: &str, req: cc::SetFeatureFlagRequest) -> Result<cc::PlatformConfig, Status> {
		Ok(self.platform().set_feature_flag(bearer(token, req)?).await?.into_inner())
	}

	// ── admin: piggybank money plane (money token) ──────────────────────────────
	pub async fn treasury(&self, token: &str) -> Result<bk::Treasury, Status> {
		Ok(self.balance().get_treasury(bearer(token, bk::GetTreasuryRequest {})?).await?.into_inner())
	}

	pub async fn admin_user_balance(&self, token: &str, user_id: &str) -> Result<bk::UserBalanceResponse, Status> {
		let req = bk::AdminBalanceRequest { user_id: user_id.to_string() };
		Ok(self.users_svc().get_user_balance(bearer(token, req)?).await?.into_inner())
	}

	pub async fn redemption_queue(&self, token: &str) -> Result<bk::RedemptionQueue, Status> {
		Ok(self.balance().list_redemption_queue(bearer(token, bk::ListRedemptionQueueRequest {})?).await?.into_inner())
	}

	pub async fn post_valuation(&self, token: &str, req: bk::PostFundValuationRequest) -> Result<bk::FundNav, Status> {
		Ok(self.balance().post_fund_valuation(bearer(token, req)?).await?.into_inner())
	}

	pub async fn settle_redemption(&self, token: &str, redemption_id: &str) -> Result<bk::Redemption, Status> {
		let req = bk::SettleRedemptionRequest {
			redemption_id: redemption_id.to_string(),
		};
		Ok(self.balance().settle_redemption(bearer(token, req)?).await?.into_inner())
	}

	pub async fn fail_redemption(&self, token: &str, redemption_id: &str) -> Result<bk::Redemption, Status> {
		let req = bk::FailRedemptionRequest {
			redemption_id: redemption_id.to_string(),
		};
		Ok(self.balance().fail_redemption(bearer(token, req)?).await?.into_inner())
	}

	pub async fn withdrawal_queue(&self, token: &str) -> Result<bk::WithdrawalQueue, Status> {
		Ok(self.balance().list_withdrawal_queue(bearer(token, bk::ListWithdrawalQueueRequest {})?).await?.into_inner())
	}

	pub async fn dispatch_withdrawal(&self, token: &str, withdrawal_id: &str) -> Result<(), Status> {
		let req = bk::DispatchWithdrawalRequest {
			withdrawal_id: withdrawal_id.to_string(),
		};
		self.balance().dispatch_withdrawal(bearer(token, req)?).await?;
		Ok(())
	}

	pub async fn settle_withdrawal(&self, token: &str, withdrawal_id: &str, tx_ref: &str) -> Result<(), Status> {
		let req = bk::SettleWithdrawalRequest {
			withdrawal_id: withdrawal_id.to_string(),
			tx_ref: tx_ref.to_string(),
		};
		self.balance().settle_withdrawal(bearer(token, req)?).await?;
		Ok(())
	}

	pub async fn fail_withdrawal(&self, token: &str, withdrawal_id: &str, reason: &str) -> Result<(), Status> {
		let req = bk::FailWithdrawalRequest {
			withdrawal_id: withdrawal_id.to_string(),
			reason: reason.to_string(),
		};
		self.balance().fail_withdrawal(bearer(token, req)?).await?;
		Ok(())
	}

	pub async fn parked_events(&self, token: &str) -> Result<bk::ParkedEventList, Status> {
		Ok(self.balance().list_parked_events(bearer(token, bk::ListParkedEventsRequest {})?).await?.into_inner())
	}

	pub async fn unpark_event(&self, token: &str, seq: i64) -> Result<(), Status> {
		let req = bk::UnparkEventRequest { seq };
		self.balance().unpark_event(bearer(token, req)?).await?;
		Ok(())
	}

	pub async fn operations_mode(&self, token: &str) -> Result<bk::OperationsMode, Status> {
		Ok(self.balance().get_operations_mode(bearer(token, bk::GetOperationsModeRequest {})?).await?.into_inner())
	}

	pub async fn set_operations_mode(&self, token: &str, read_only: bool) -> Result<bk::OperationsMode, Status> {
		let req = bk::SetOperationsModeRequest { read_only };
		Ok(self.balance().set_operations_mode(bearer(token, req)?).await?.into_inner())
	}
}

/// A lazily-connected upstream `Endpoint` with explicit connect + per-RPC deadlines, so a
/// degraded plane fails fast instead of stalling the awaiting task indefinitely.
fn endpoint(addr: &str) -> anyhow::Result<Endpoint> {
	Ok(Endpoint::from_shared(addr.to_string())?.connect_timeout(CONNECT_TIMEOUT).timeout(REQUEST_TIMEOUT))
}

/// Attach the user's access token as `authorization: Bearer …` metadata — the inbound
/// auth layer on each plane reads exactly that.
// `tonic::Status` is a large error type; boxing it at every call site buys nothing here.
#[allow(clippy::result_large_err)]
fn bearer<T>(token: &str, msg: T) -> Result<Request<T>, Status> {
	let mut req = Request::new(msg);
	let value = format!("Bearer {token}").parse().map_err(|_| Status::unauthenticated("invalid bearer token"))?;
	req.metadata_mut().insert("authorization", value);
	Ok(req)
}

#[cfg(test)]
mod tests {
	use std::{net::TcpListener, time::Instant};

	use super::*;

	/// A half-open upstream — a port whose TCP connection completes but never speaks HTTP/2
	/// — is the worst case for the missing timeout: the connect succeeds, then the RPC awaits
	/// a response that never comes. A bound listener that we never `accept()` is exactly that:
	/// the OS backlog completes the handshake, but no gRPC frame is ever served. With the
	/// per-RPC `timeout` on the `Endpoint`, the call must return an error well inside the
	/// layer's outer deadline rather than hanging; an outer guard fails loudly on regression.
	#[tokio::test]
	async fn upstream_rpc_fails_fast_against_a_half_open_plane() {
		let listener = TcpListener::bind("127.0.0.1:0").expect("bind black-hole listener");
		let addr = listener.local_addr().unwrap();

		let grpc = Grpc::connect_lazy(&format!("http://{addr}"), &format!("http://{addr}"), &format!("http://{addr}"), None).expect("build lazy channels");

		let started = Instant::now();
		let guard = tokio::time::timeout(REQUEST_TIMEOUT + Duration::from_secs(5), grpc.check()).await;

		let result = guard.expect("the call must return within the deadline, not hang");
		assert!(result.is_err(), "a half-open plane must surface an error, not a hung await");
		assert!(started.elapsed() < REQUEST_TIMEOUT + Duration::from_secs(2), "the per-RPC timeout must bound the call");

		drop(listener);
	}
}

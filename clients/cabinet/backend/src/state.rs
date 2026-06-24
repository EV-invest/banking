use std::sync::Arc;

use evbanking_contracts::banking::v1 as bk;
use evconcierge_contracts::concierge::v1 as cc;
use tonic::{
	Request, Status,
	transport::{Channel, Endpoint},
};

use crate::{config::Config, cookies::CookieNames, oauth::OAuthTxStore, session::SessionStore};

/// Shared application state, cheaply cloneable per request.
#[derive(Clone)]
pub struct AppState {
	pub config: Arc<Config>,
	pub grpc: Grpc,
	pub sessions: Arc<SessionStore>,
	pub oauth: Arc<OAuthTxStore>,
	pub cookies: Arc<CookieNames>,
}

/// gRPC egress to both planes. Channels are lazily connected and cheap to clone, so a
/// fresh typed client is built per call from the shared channel.
#[derive(Clone)]
pub struct Grpc {
	piggybank: Channel,
	concierge: Channel,
}
impl Grpc {
	pub fn connect_lazy(piggybank_addr: &str, concierge_addr: &str) -> anyhow::Result<Self> {
		let piggybank = Endpoint::from_shared(piggybank_addr.to_string())?.connect_lazy();
		let concierge = Endpoint::from_shared(concierge_addr.to_string())?.connect_lazy();
		Ok(Self { piggybank, concierge })
	}

	fn auth(&self) -> cc::auth_service_client::AuthServiceClient<Channel> {
		cc::auth_service_client::AuthServiceClient::new(self.concierge.clone())
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

	// ── concierge identity plane ───────────────────────────────────────────────
	pub async fn exchange(&self, req: cc::ExchangeRequest) -> Result<cc::TokenResponse, Status> {
		Ok(self.auth().exchange(req).await?.into_inner())
	}

	pub async fn refresh(&self, refresh_token: &str) -> Result<cc::TokenResponse, Status> {
		let req = cc::RefreshRequest {
			refresh_token: refresh_token.to_string(),
		};
		Ok(self.auth().refresh(req).await?.into_inner())
	}

	pub async fn logout(&self, refresh_token: &str, revoke_all: bool) -> Result<(), Status> {
		let req = cc::LogoutRequest {
			refresh_token: refresh_token.to_string(),
			revoke_all,
		};
		self.auth().logout(req).await?;
		Ok(())
	}

	pub async fn list_sessions(&self, refresh_token: &str) -> Result<cc::ListSessionsResponse, Status> {
		let req = cc::ListSessionsRequest {
			refresh_token: refresh_token.to_string(),
		};
		Ok(self.auth().list_sessions(req).await?.into_inner())
	}

	pub async fn revoke_session(&self, refresh_token: &str, session_id: &str) -> Result<(), Status> {
		let req = cc::RevokeSessionRequest {
			refresh_token: refresh_token.to_string(),
			session_id: session_id.to_string(),
		};
		self.auth().revoke_session(req).await?;
		Ok(())
	}

	pub async fn get_me(&self, token: &str) -> Result<cc::UserProfile, Status> {
		Ok(self.directory().get_me(bearer(token, cc::GetMeRequest {})?).await?.into_inner())
	}

	pub async fn update_profile(&self, token: &str, req: cc::UpdateProfileRequest) -> Result<cc::UserProfile, Status> {
		Ok(self.directory().update_profile(bearer(token, req)?).await?.into_inner())
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

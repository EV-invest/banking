//! `funds` context — a user's fund investments via the service currency (units/shares).
//!
//! Subscribe/redeem/cancel act on the caller's own `sub`; the queue settlement and
//! valuation operator RPCs live on [`BalanceSvc`](super::balance::BalanceSvc).
//!
//! `Result<_, Status>` is tonic's mandated handler signature; `Status` is a large
//! type we don't control, so the large-err lint does not apply in this module.
#![allow(clippy::result_large_err)]

use domain::{
	balance::ServiceId,
	money::{Shares, Usdt},
	redemptions::Redemption,
	subscriptions::Subscription,
};
use evbanking_contracts::banking::v1::{self as pb, funds_service_server::FundsService};
use tonic::{Request, Response, Status};

use crate::{
	AppState,
	application::funds as funds_app,
	services::support::{caller_id, map_err, parse_redemption_id, unix_now},
};

#[derive(Clone)]
pub struct FundsSvc {
	pub state: AppState,
}

impl FundsSvc {
	pub fn new(state: AppState) -> Self {
		Self { state }
	}
}

#[tonic::async_trait]
impl FundsService for FundsSvc {
	async fn subscribe(&self, request: Request<pb::SubscribeRequest>) -> Result<Response<pb::Subscription>, Status> {
		let user = caller_id(&request)?;
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		let amount = Usdt::parse_decimal(&req.amount).map_err(map_err)?;
		let subscription = funds_app::subscribe(
			self.state.subscriptions.as_ref(),
			self.state.ledger.as_ref(),
			self.state.nav.as_ref(),
			&self.state.relay_notify,
			user,
			service,
			amount,
			unix_now(),
		)
		.await
		.map_err(map_err)?;
		Ok(Response::new(subscription_to_proto(&subscription)))
	}

	async fn redeem(&self, request: Request<pb::RedeemRequest>) -> Result<Response<pb::Redemption>, Status> {
		let user = caller_id(&request)?;
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		let units = Shares::parse_decimal(&req.units).map_err(map_err)?;
		let redemption = funds_app::request_redemption(
			self.state.redemptions.as_ref(),
			self.state.ledger.as_ref(),
			self.state.nav.as_ref(),
			&self.state.relay_notify,
			user,
			service,
			units,
			unix_now(),
		)
		.await
		.map_err(map_err)?;
		Ok(Response::new(redemption_to_proto(&redemption)))
	}

	async fn cancel_redemption(&self, request: Request<pb::CancelRedemptionRequest>) -> Result<Response<pb::Redemption>, Status> {
		let user = caller_id(&request)?;
		let id = parse_redemption_id(&request.get_ref().redemption_id)?;
		let redemption = funds_app::cancel_redemption(self.state.redemptions.as_ref(), &self.state.relay_notify, id, user)
			.await
			.map_err(map_err)?;
		Ok(Response::new(redemption_to_proto(&redemption)))
	}

	async fn get_position(&self, request: Request<pb::GetPositionRequest>) -> Result<Response<pb::Position>, Status> {
		let user = caller_id(&request)?;
		let service = ServiceId::parse(&request.get_ref().service).map_err(map_err)?;
		let view = funds_app::get_position(self.state.positions.as_ref(), self.state.ledger.as_ref(), self.state.nav.as_ref(), user, service)
			.await
			.map_err(map_err)?;
		Ok(Response::new(position_to_proto(&view)))
	}

	async fn list_positions(&self, request: Request<pb::ListPositionsRequest>) -> Result<Response<pb::PositionList>, Status> {
		let user = caller_id(&request)?;
		let views = funds_app::list_positions(self.state.positions.as_ref(), self.state.ledger.as_ref(), self.state.nav.as_ref(), user)
			.await
			.map_err(map_err)?;
		Ok(Response::new(pb::PositionList {
			positions: views.iter().map(position_to_proto).collect(),
		}))
	}

	async fn list_redemptions(&self, request: Request<pb::ListRedemptionsRequest>) -> Result<Response<pb::RedemptionList>, Status> {
		let user = caller_id(&request)?;
		let redemptions = funds_app::list_redemptions(self.state.redemptions.as_ref(), user).await.map_err(map_err)?;
		Ok(Response::new(pb::RedemptionList {
			redemptions: redemptions.iter().map(redemption_to_proto).collect(),
		}))
	}

	async fn get_fund_nav(&self, request: Request<pb::GetFundNavRequest>) -> Result<Response<pb::FundNav>, Status> {
		// Any authenticated user may read a fund's price.
		caller_id(&request)?;
		let service = ServiceId::parse(&request.get_ref().service).map_err(map_err)?;
		let view = funds_app::fund_nav_view(self.state.nav.as_ref(), self.state.ledger.as_ref(), service, unix_now())
			.await
			.map_err(map_err)?;
		Ok(Response::new(fund_nav_to_proto(&view)))
	}
}

fn subscription_to_proto(subscription: &Subscription) -> pb::Subscription {
	pb::Subscription {
		id: subscription.id().to_string(),
		service: subscription.service().to_string(),
		cash: subscription.cash().to_decimal_string(),
		nav: subscription.nav().to_decimal_string(),
		units: subscription.units().to_decimal_string(),
	}
}

/// Shared with [`BalanceSvc`](super::balance::BalanceSvc)'s settlement RPCs.
pub(super) fn redemption_to_proto(redemption: &Redemption) -> pb::Redemption {
	pb::Redemption {
		id: redemption.id().to_string(),
		service: redemption.service().to_string(),
		units: redemption.units().to_decimal_string(),
		nav: redemption.nav().map(|n| n.to_decimal_string()).unwrap_or_default(),
		cash: redemption.cash().map(|c| c.to_decimal_string()).unwrap_or_default(),
		state: redemption.state().as_str().to_owned(),
	}
}

fn position_to_proto(view: &funds_app::PositionView) -> pb::Position {
	pb::Position {
		service: view.service.to_string(),
		units: view.units.to_decimal_string(),
		nav: view.nav.to_decimal_string(),
		value: view.value.to_decimal_string(),
		cost_basis: view.cost_basis.to_decimal_string(),
		pnl: signed_diff(view.value, view.cost_basis),
		nav_as_of: view.nav_as_of,
	}
}

fn fund_nav_to_proto(view: &funds_app::FundNavView) -> pb::FundNav {
	pb::FundNav {
		service: view.service.to_string(),
		nav: view.nav.to_decimal_string(),
		aum: view.aum.map(|a| a.to_decimal_string()).unwrap_or_default(),
		units_outstanding: view.units_outstanding.to_decimal_string(),
		posted_at: view.posted_at,
		stale: view.stale,
	}
}

/// `value − cost_basis` as a signed decimal string (P&L; negative on a loss). `Usdt` is
/// unsigned, so the sign is applied here at the wire boundary.
fn signed_diff(value: Usdt, cost_basis: Usdt) -> String {
	if value >= cost_basis {
		value.checked_sub(cost_basis).unwrap_or(Usdt::ZERO).to_decimal_string()
	} else {
		format!("-{}", cost_basis.checked_sub(value).unwrap_or(Usdt::ZERO).to_decimal_string())
	}
}

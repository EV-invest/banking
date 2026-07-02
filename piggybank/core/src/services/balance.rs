//! `balance` context — company-money RPCs (all admin-gated): treasury reads,
//! capital seeding/deposit recording, the operator withdrawal lifecycle, and fund
//! valuation + redemption settlement.
//!
//! `Result<_, Status>` is tonic's mandated handler signature; `Status` is a large
//! type we don't control, so the large-err lint does not apply in this module.
#![allow(clippy::result_large_err)]

use domain::{
	authz::Permission,
	balance::{Party, ServiceId},
	money::{Network, TxRef, Usdt},
};
use evbanking_auth::claims_of;
use evbanking_contracts::banking::v1::{self as pb, balance_service_server::BalanceService};
use tonic::{Request, Response, Status};

use crate::{
	AppState,
	application::{balance as balance_app, funds as funds_app, withdrawals as withdrawal_app},
	services::{
		funds::redemption_to_proto,
		support::{map_err, optional, parse_redemption_id, parse_withdrawal_id, require_permission, unix_now},
	},
};

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
	async fn get_treasury(&self, request: Request<pb::GetTreasuryRequest>) -> Result<Response<pb::Treasury>, Status> {
		require_permission(&self.state, &request, Permission::TreasuryRead).await?;
		let t = balance_app::treasury(self.state.ledger.as_ref()).await.map_err(map_err)?;
		Ok(Response::new(pb::Treasury {
			rails: t
				.rails
				.into_iter()
				.map(|r| pb::RailLiquidity {
					network: r.network.as_str().to_owned(),
					custody: r.custody.to_decimal_string(),
				})
				.collect(),
			bank: t.bank.to_decimal_string(),
			total_custody: t.total_custody.to_decimal_string(),
			fund_capital: t.fund_capital.to_decimal_string(),
			fee_revenue: t.fee_revenue.to_decimal_string(),
			held_for_clients: t.held_for_clients.to_decimal_string(),
			reserved_for_withdrawals: t.reserved_for_withdrawals.to_decimal_string(),
		}))
	}

	async fn seed_capital(&self, request: Request<pb::SeedCapitalRequest>) -> Result<Response<pb::SeedCapitalResponse>, Status> {
		require_permission(&self.state, &request, Permission::CapitalManage).await?;
		let req = request.into_inner();
		let network = Network::parse(&req.network).map_err(map_err)?;
		let amount = Usdt::parse_decimal(&req.amount).map_err(map_err)?;
		balance_app::seed_fund_capital(self.state.deposits.as_ref(), &self.state.relay_notify, network, amount)
			.await
			.map_err(map_err)?;
		Ok(Response::new(pb::SeedCapitalResponse {}))
	}

	async fn record_deposit(&self, request: Request<pb::RecordDepositRequest>) -> Result<Response<pb::RecordDepositResponse>, Status> {
		require_permission(&self.state, &request, Permission::CapitalManage).await?;
		let req = request.into_inner();
		let tx_ref = TxRef::parse(&req.tx_ref).map_err(map_err)?;
		let network = Network::parse(&req.network).map_err(map_err)?;
		let amount = Usdt::parse_decimal(&req.amount).map_err(map_err)?;
		let party = Party::from_parts(&req.party_kind, optional(&req.party_id)).map_err(map_err)?;
		let recorded = balance_app::record_deposit(self.state.deposits.as_ref(), &self.state.relay_notify, tx_ref, party, network, amount)
			.await
			.map_err(map_err)?;
		Ok(Response::new(pb::RecordDepositResponse { recorded }))
	}

	async fn dispatch_withdrawal(&self, request: Request<pb::DispatchWithdrawalRequest>) -> Result<Response<pb::DispatchWithdrawalResponse>, Status> {
		require_permission(&self.state, &request, Permission::WithdrawalDispatch).await?;
		let id = parse_withdrawal_id(&request.get_ref().withdrawal_id)?;
		withdrawal_app::dispatch_withdrawal(self.state.withdrawals.as_ref(), &self.state.relay_notify, id)
			.await
			.map_err(map_err)?;
		Ok(Response::new(pb::DispatchWithdrawalResponse {}))
	}

	async fn settle_withdrawal(&self, request: Request<pb::SettleWithdrawalRequest>) -> Result<Response<pb::SettleWithdrawalResponse>, Status> {
		require_permission(&self.state, &request, Permission::WithdrawalSettle).await?;
		let req = request.into_inner();
		let id = parse_withdrawal_id(&req.withdrawal_id)?;
		let tx_ref = TxRef::parse(&req.tx_ref).map_err(map_err)?;
		withdrawal_app::settle_withdrawal(self.state.withdrawals.as_ref(), &self.state.relay_notify, id, tx_ref)
			.await
			.map_err(map_err)?;
		Ok(Response::new(pb::SettleWithdrawalResponse {}))
	}

	async fn fail_withdrawal(&self, request: Request<pb::FailWithdrawalRequest>) -> Result<Response<pb::FailWithdrawalResponse>, Status> {
		require_permission(&self.state, &request, Permission::WithdrawalFail).await?;
		let id = parse_withdrawal_id(&request.get_ref().withdrawal_id)?;
		withdrawal_app::fail_withdrawal(self.state.withdrawals.as_ref(), &self.state.relay_notify, id)
			.await
			.map_err(map_err)?;
		Ok(Response::new(pb::FailWithdrawalResponse {}))
	}

	async fn post_fund_valuation(&self, request: Request<pb::PostFundValuationRequest>) -> Result<Response<pb::FundNav>, Status> {
		require_permission(&self.state, &request, Permission::ValuationPost).await?;
		let claims = claims_of(&request).ok_or_else(|| Status::unauthenticated("missing claims"))?;
		let posted_by = claims.sub.clone();
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		let aum = Usdt::parse_decimal(&req.aum).map_err(map_err)?;
		let valuation = funds_app::post_fund_valuation(self.state.nav.as_ref(), self.state.ledger.as_ref(), service.clone(), aum, &posted_by, req.r#override)
			.await
			.map_err(map_err)?;
		Ok(Response::new(pb::FundNav {
			service: service.to_string(),
			nav: valuation.nav.to_decimal_string(),
			aum: valuation.aum.to_decimal_string(),
			units_outstanding: valuation.units_outstanding.to_decimal_string(),
			posted_at: valuation.posted_at_unix,
			stale: false,
		}))
	}

	async fn settle_redemption(&self, request: Request<pb::SettleRedemptionRequest>) -> Result<Response<pb::Redemption>, Status> {
		require_permission(&self.state, &request, Permission::RedemptionSettle).await?;
		let id = parse_redemption_id(&request.get_ref().redemption_id)?;
		let redemption = funds_app::settle_redemption(self.state.redemptions.as_ref(), self.state.nav.as_ref(), &self.state.relay_notify, id, unix_now())
			.await
			.map_err(map_err)?;
		Ok(Response::new(redemption_to_proto(&redemption)))
	}

	async fn fail_redemption(&self, request: Request<pb::FailRedemptionRequest>) -> Result<Response<pb::Redemption>, Status> {
		require_permission(&self.state, &request, Permission::RedemptionFail).await?;
		let id = parse_redemption_id(&request.get_ref().redemption_id)?;
		let redemption = funds_app::fail_redemption(self.state.redemptions.as_ref(), &self.state.relay_notify, id).await.map_err(map_err)?;
		Ok(Response::new(redemption_to_proto(&redemption)))
	}

	async fn list_redemption_queue(&self, request: Request<pb::ListRedemptionQueueRequest>) -> Result<Response<pb::RedemptionQueue>, Status> {
		require_permission(&self.state, &request, Permission::RedemptionSettle).await?;
		let queued = self.state.redemptions.list_queued().await.map_err(map_err)?;
		Ok(Response::new(pb::RedemptionQueue {
			items: queued
				.into_iter()
				.map(|q| pb::RedemptionQueueItem {
					redemption_id: q.id.to_string(),
					user_id: q.user_id.to_string(),
					email: q.email,
					service: q.service.to_string(),
					units: q.units.to_decimal_string(),
					created_at: q.created_at,
				})
				.collect(),
		}))
	}

	async fn get_operations_mode(&self, request: Request<pb::GetOperationsModeRequest>) -> Result<Response<pb::OperationsMode>, Status> {
		require_permission(&self.state, &request, Permission::TreasuryRead).await?;
		let read_only = crate::infrastructure::operations::is_read_only(&self.state.pool)
			.await
			.map_err(|_| Status::unavailable("internal error"))?;
		Ok(Response::new(pb::OperationsMode { read_only }))
	}

	async fn set_operations_mode(&self, request: Request<pb::SetOperationsModeRequest>) -> Result<Response<pb::OperationsMode>, Status> {
		require_permission(&self.state, &request, Permission::OperationsManage).await?;
		let read_only = crate::infrastructure::operations::set_read_only(&self.state.pool, request.get_ref().read_only)
			.await
			.map_err(|_| Status::unavailable("internal error"))?;
		Ok(Response::new(pb::OperationsMode { read_only }))
	}
}

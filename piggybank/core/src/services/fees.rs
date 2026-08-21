//! `fees` context — a fund's management and performance fee.
//!
//! Reads of a fund's *terms* are open to any authenticated user: an investor is
//! entitled to know what they are paying before they pay it, and a policy is public
//! information about a product. Reads of a *charge* are scoped to the caller's own
//! statement. Everything that sets terms or moves value is gated on
//! [`Permission::AllocationManage`] — the same trust seam as registering the product
//! or posting its valuation, because a fee policy is part of what the product *is*.
//!
//! Only [`SettleFeeShares`](FeesService::settle_fee_shares) moves money, and it
//! notifies the relay. Charging happens on the sweeper, not on this surface: a fee is
//! a consequence of time passing, never of somebody calling an RPC.
//!
//! `Result<_, Status>` is tonic's mandated handler signature; `Status` is a large type
//! we don't control, so the large-err lint does not apply in this module.
#![allow(clippy::result_large_err)]

use domain::{
	authz::Permission,
	balance::ServiceId,
	fees::{CrystallizationPeriod, FeePolicy, ManagementBasis},
	money::Shares,
};
use evbanking_auth::claims_of;
use evbanking_contracts::banking::v1::{self as pb, fees_service_server::FeesService};
use tonic::{Request, Response, Status};

use crate::{
	AppState,
	application::fees as fee_app,
	ports::fees::AssessmentRecord,
	services::support::{caller_id, map_err, require_permission, unix_now},
};

#[derive(Clone)]
pub struct FeesSvc {
	pub state: AppState,
}

impl FeesSvc {
	pub fn new(state: AppState) -> Self {
		Self { state }
	}
}

#[tonic::async_trait]
impl FeesService for FeesSvc {
	async fn get_fee_policy(&self, request: Request<pb::GetFeePolicyRequest>) -> Result<Response<pb::FeePolicy>, Status> {
		caller_id(&request)?;
		let service = ServiceId::parse(&request.get_ref().service).map_err(map_err)?;
		let policy = fee_app::get_policy(self.state.fees.policies.as_ref(), &service).await.map_err(map_err)?;
		Ok(Response::new(policy_to_proto(&service, policy)))
	}

	async fn list_fee_policies(&self, request: Request<pb::ListFeePoliciesRequest>) -> Result<Response<pb::FeePolicyList>, Status> {
		caller_id(&request)?;
		let policies = fee_app::list_policies(self.state.fees.policies.as_ref()).await.map_err(map_err)?;
		Ok(Response::new(pb::FeePolicyList {
			policies: policies.iter().map(|(service, policy)| policy_to_proto(service, Some(*policy))).collect(),
		}))
	}

	async fn set_fee_policy(&self, request: Request<pb::SetFeePolicyRequest>) -> Result<Response<pb::FeePolicy>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		// The operator subject is the trust seam on the terms, exactly as `posted_by` is on
		// a valuation: the audit answers "who set this fee", not just "the fee changed".
		let updated_by = claims_of(&request).ok_or_else(|| Status::unauthenticated("missing claims"))?.sub.clone();
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		// Parsed at the boundary so a bad rate or an unknown vocabulary word is an
		// `invalid_argument` about the input, not a validation error from deeper in.
		let policy = FeePolicy::new(
			req.management_bps,
			req.performance_bps,
			req.hurdle_bps,
			ManagementBasis::parse(&req.basis).map_err(map_err)?,
			CrystallizationPeriod::parse(&req.crystallization).map_err(map_err)?,
		)
		.map_err(map_err)?;
		let stored = fee_app::set_policy(self.state.fees.policies.as_ref(), self.state.allocations.as_ref(), service.clone(), policy, &updated_by)
			.await
			.map_err(map_err)?;
		Ok(Response::new(policy_to_proto(&service, Some(stored))))
	}

	async fn list_fee_assessments(&self, request: Request<pb::ListFeeAssessmentsRequest>) -> Result<Response<pb::FeeAssessmentList>, Status> {
		let caller = caller_id(&request)?;
		let records = fee_app::list_assessments(self.state.fees.assessments.as_ref(), caller).await.map_err(map_err)?;
		Ok(Response::new(pb::FeeAssessmentList {
			assessments: records.iter().map(assessment_to_proto).collect(),
		}))
	}

	async fn list_fund_fee_assessments(&self, request: Request<pb::ListFundFeeAssessmentsRequest>) -> Result<Response<pb::FeeAssessmentList>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		let service = ServiceId::parse(&request.get_ref().service).map_err(map_err)?;
		let records = fee_app::list_fund_assessments(self.state.fees.assessments.as_ref(), &service).await.map_err(map_err)?;
		Ok(Response::new(pb::FeeAssessmentList {
			assessments: records.iter().map(assessment_to_proto).collect(),
		}))
	}

	async fn get_accrued_fees(&self, request: Request<pb::GetAccruedFeesRequest>) -> Result<Response<pb::AccruedFees>, Status> {
		let caller = caller_id(&request)?;
		let service = ServiceId::parse(&request.get_ref().service).map_err(map_err)?;
		let accrued = fee_app::accrued_fees(
			self.state.fees.policies.as_ref(),
			self.state.fees.accruals.as_ref(),
			self.state.ledger.as_ref(),
			self.state.nav.as_ref(),
			caller,
			&service,
			unix_now(),
		)
		.await
		.map_err(map_err)?;
		Ok(Response::new(match accrued {
			Some(accrued) => pb::AccruedFees {
				service: service.to_string(),
				configured: true,
				management: accrued.management.to_decimal_string(),
				performance: accrued.performance.to_decimal_string(),
				debt: accrued.debt.to_decimal_string(),
				total: accrued.total.to_decimal_string(),
				high_water_mark: accrued.high_water_mark.to_decimal_string(),
			},
			// A fund with no policy, or an investor with no position: zeros, and
			// `configured = false` so a client can tell "charges nothing" from "owes
			// nothing right now".
			None => pb::AccruedFees {
				service: service.to_string(),
				configured: false,
				management: "0".into(),
				performance: "0".into(),
				debt: "0".into(),
				total: "0".into(),
				high_water_mark: "0".into(),
			},
		}))
	}

	async fn get_fee_shares(&self, request: Request<pb::GetFeeSharesRequest>) -> Result<Response<pb::FeeShares>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		let service = ServiceId::parse(&request.get_ref().service).map_err(map_err)?;
		let (units, value) = fee_app::fee_shares(self.state.ledger.as_ref(), self.state.nav.as_ref(), &service, unix_now())
			.await
			.map_err(map_err)?;
		Ok(Response::new(pb::FeeShares {
			service: service.to_string(),
			units: units.to_decimal_string(),
			value: value.to_decimal_string(),
		}))
	}

	async fn settle_fee_shares(&self, request: Request<pb::SettleFeeSharesRequest>) -> Result<Response<pb::FeeSettlement>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		let settled_by = claims_of(&request).ok_or_else(|| Status::unauthenticated("missing claims"))?.sub.clone();
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		// Empty means "all of it" — the ordinary end-of-period call.
		let units = match req.units.trim() {
			"" => None,
			raw => Some(Shares::parse_decimal(raw).map_err(map_err)?),
		};
		let settlement = fee_app::settle_fee_shares(
			self.state.fees.settlements.as_ref(),
			self.state.ledger.as_ref(),
			self.state.nav.as_ref(),
			self.state.redemptions.as_ref(),
			&self.state.relay_notify,
			service,
			units,
			&settled_by,
			unix_now(),
		)
		.await
		.map_err(map_err)?;
		Ok(Response::new(pb::FeeSettlement {
			service: settlement.service().to_string(),
			units: settlement.units().to_decimal_string(),
			nav: settlement.nav().to_decimal_string(),
			cash: settlement.cash().to_decimal_string(),
		}))
	}
}

fn policy_to_proto(service: &ServiceId, policy: Option<FeePolicy>) -> pb::FeePolicy {
	match policy {
		Some(policy) => pb::FeePolicy {
			service: service.to_string(),
			configured: true,
			management_bps: policy.management_bps(),
			performance_bps: policy.performance_bps(),
			hurdle_bps: policy.hurdle_bps(),
			basis: policy.basis().as_str().to_owned(),
			crystallization: policy.crystallization().as_str().to_owned(),
			updated_at: 0,
		},
		// Not an error and not a 404: "this product charges no fee" is a real answer, and
		// the client renders it as such rather than as a missing policy.
		None => pb::FeePolicy {
			service: service.to_string(),
			configured: false,
			management_bps: 0,
			performance_bps: 0,
			hurdle_bps: 0,
			basis: ManagementBasis::InvestedCapital.as_str().to_owned(),
			crystallization: CrystallizationPeriod::Annual.as_str().to_owned(),
			updated_at: 0,
		},
	}
}

fn assessment_to_proto(record: &AssessmentRecord) -> pb::FeeAssessment {
	pb::FeeAssessment {
		service: record.service.to_string(),
		trigger: record.trigger.as_str().to_owned(),
		nav: record.nav.to_decimal_string(),
		management: record.management.to_decimal_string(),
		performance: record.performance.to_decimal_string(),
		debt_opening: record.debt_opening.to_decimal_string(),
		charged_units: record.charged_units.to_decimal_string(),
		charged_cash: record.charged_cash.to_decimal_string(),
		debt_carried: record.debt_carried.to_decimal_string(),
		high_water_mark: record.high_water_mark.to_decimal_string(),
		assessed_at: record.assessed_at_unix,
	}
}

//! `balance` context — company-money RPCs (all admin-gated): treasury reads,
//! chain-proven arrival recording (a deposit; a seed of the `fund` allocation opens the
//! owners' consilium instead of booking), the operator withdrawal lifecycle, and fund
//! valuation + redemption settlement.
//!
//! `Result<_, Status>` is tonic's mandated handler signature; `Status` is a large
//! type we don't control, so the large-err lint does not apply in this module.
#![allow(clippy::result_large_err)]

use domain::{
	authz::Permission,
	balance::{Party, ServiceId},
	consilium::SeedCapitalTerms,
	money::{Network, TxRef, Usdt},
};
use evbanking_auth::claims_of;
use evbanking_contracts::banking::v1::{self as pb, balance_service_server::BalanceService};
use tonic::{Request, Response, Status};

use crate::{
	AppState,
	application::{balance as balance_app, consilium as consilium_app, funds as funds_app, wallet as wallet_app, withdrawals as withdrawal_app},
	services::{
		funds::redemption_to_proto,
		support::{caller_id, map_err, optional, parse_redemption_id, parse_user_id, parse_withdrawal_id, rail_is_testnet, require_permission, unix_now},
		wallet::withdrawal_to_proto,
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
		let t = balance_app::treasury(self.state.ledger.as_ref(), self.state.custody.as_ref()).await.map_err(map_err)?;
		Ok(Response::new(pb::Treasury {
			rails: t
				.rails
				.into_iter()
				.map(|r| pb::RailLiquidity {
					is_testnet: rail_is_testnet(&self.state, r.network),
					network: r.network.as_str().to_owned(),
					custody: r.custody.to_decimal_string(),
					treasury_address: r.treasury_address.unwrap_or_default(),
					onchain_usdt: r.onchain_usdt.map(Usdt::to_decimal_string).unwrap_or_default(),
					onchain_gas: r.onchain_gas.unwrap_or_default(),
					gas_station_address: r.gas_station_address.unwrap_or_default(),
					gas_station_gas: r.gas_station_gas.unwrap_or_default(),
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

	/// Propose a seed of the platform's capital: the caller's chain-proven transfer into
	/// the treasury, to be booked as THEIR deposit and subscribed into the `fund`
	/// allocation once the owners' quorum executes it (#245).
	///
	/// This OPENS A CONSILIUM and books nothing. The chain proves the dollar arrived on the
	/// treasury; it cannot say whose it is, and the first administrator to name a reference
	/// must not be the one who decides — so the attribution is the owners' call, and the
	/// caller must hold a seat (`Consilium::open` refuses anyone else, whatever permission
	/// admitted them here). The arrival is verified now, against `expected_amount`, so the
	/// owners vote over a transfer that exists and is worth what the terms say.
	///
	/// The depositor is the caller: the wire carries no `depositor_user_id` yet (the
	/// contract step of #245, C-7, adds one so an owner can attribute another person's
	/// transfer, and puts the `consilium_id` in the response — until then it is in the
	/// log line). `expected_amount` is REQUIRED: the amount is under the owners'
	/// signature, so "whatever the chain says" is not a proposal. The response reports
	/// `recorded = false` — nothing is booked until the quorum executes — and the amount
	/// the terms carry.
	async fn seed_capital(&self, request: Request<pb::SeedCapitalRequest>) -> Result<Response<pb::SeedCapitalResponse>, Status> {
		require_permission(&self.state, &request, Permission::CapitalManage).await?;
		let depositor = caller_id(&request)?;
		let req = request.into_inner();
		let tx_ref = TxRef::parse(&req.tx_ref).map_err(map_err)?;
		let network = Network::parse(&req.network).map_err(map_err)?;
		let amount = optional(&req.expected_amount)
			.ok_or_else(|| Status::invalid_argument("expected_amount is required: a seed is proposed at the amount the chain reports, and the owners approve that figure"))
			.and_then(|raw| Usdt::parse_decimal(raw).map_err(map_err))?;
		let terms = SeedCapitalTerms::new(tx_ref, network, amount, depositor).map_err(map_err)?;
		let opened = consilium_app::open_seed_capital(&self.state.consilium_ports(), depositor, terms, unix_now())
			.await
			.map_err(map_err)?;
		tracing::info!(
			consilium_id = %opened.consilium.id(),
			%depositor,
			amount = %amount.to_decimal_string(),
			"seed consilium opened: the treasury arrival is booked only once the owners' quorum executes it"
		);
		Ok(Response::new(pb::SeedCapitalResponse {
			recorded: false,
			amount: amount.to_decimal_string(),
		}))
	}

	async fn record_deposit(&self, request: Request<pb::RecordDepositRequest>) -> Result<Response<pb::RecordDepositResponse>, Status> {
		require_permission(&self.state, &request, Permission::CapitalManage).await?;
		let req = request.into_inner();
		let tx_ref = TxRef::parse(&req.tx_ref).map_err(map_err)?;
		let network = Network::parse(&req.network).map_err(map_err)?;
		// Empty means "whatever the chain says"; a value is an assertion the chain must match.
		let expected_amount = optional(&req.expected_amount).map(Usdt::parse_decimal).transpose().map_err(map_err)?;
		let arrival = balance_app::record_verified_arrival(
			self.state.deposits.as_ref(),
			self.state.custody.as_ref(),
			self.state.deposit_addresses.as_ref(),
			&self.state.relay_notify,
			tx_ref,
			network,
			expected_amount,
		)
		.await
		.map_err(map_err)?;
		// Always a person: the chain names a deposit address's owner, and a treasury
		// arrival is refused upstream (it is attributed by hand, through `SeedCapital`).
		let party = Party::User(arrival.user);
		Ok(Response::new(pb::RecordDepositResponse {
			recorded: arrival.recorded,
			amount: arrival.amount.to_decimal_string(),
			party_kind: party.kind_str().to_owned(),
			party_id: party.id_str().unwrap_or_default(),
		}))
	}

	/// Push a queued withdrawal onto its rail by operator command.
	///
	/// The permission is necessary and not sufficient: the command re-runs the outflow
	/// policy — read-only kill-switch, owner freeze, tier-1 floor — so this handle cannot
	/// ship a payout the user path and the sweep would both refuse. Deliberately NO
	/// per-call `force` override: the operator override for a pause already exists and is
	/// `SetOperationsMode`, which is itself permissioned and leaves one auditable record of
	/// who reopened outflows and when, rather than a flag on an individual payout that
	/// looks identical to an ordinary dispatch in the log.
	async fn dispatch_withdrawal(&self, request: Request<pb::DispatchWithdrawalRequest>) -> Result<Response<pb::DispatchWithdrawalResponse>, Status> {
		require_permission(&self.state, &request, Permission::WithdrawalDispatch).await?;
		let id = parse_withdrawal_id(&request.get_ref().withdrawal_id)?;
		withdrawal_app::dispatch_withdrawal(
			self.state.withdrawals.as_ref(),
			self.state.custody.as_ref(),
			self.state.outflow.as_ref(),
			self.state.kyc_gate,
			&self.state.relay_notify,
			id,
		)
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
		let caller = caller_id(&request)?;
		let claims = claims_of(&request).ok_or_else(|| Status::unauthenticated("missing claims"))?;
		let posted_by = claims.sub.clone();
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		let aum = Usdt::parse_decimal(&req.aum).map_err(map_err)?;
		funds_app::post_fund_valuation(
			self.state.allocations.as_ref(),
			self.state.nav.as_ref(),
			self.state.ledger.as_ref(),
			service.clone(),
			aum,
			&posted_by,
			unix_now(),
		)
		.await
		.map_err(map_err)?;
		// Answer by re-reading the view rather than mapping the mark by hand: the response
		// carries the allocation's supply headroom too, and one construction path is what
		// keeps this route and `GetFundNav` from drifting apart field by field. Unrestricted:
		// ValuationPost already admitted the caller to a product in any state, and the mark
		// they just wrote must not vanish behind their own (possibly `hidden`) access level.
		let view = funds_app::fund_nav_view(
			self.state.allocations.as_ref(),
			self.state.nav.as_ref(),
			self.state.ledger.as_ref(),
			service,
			caller,
			true,
			unix_now(),
		)
		.await
		.map_err(map_err)?;
		Ok(Response::new(super::funds::fund_nav_to_proto(&view)))
	}

	async fn settle_redemption(&self, request: Request<pb::SettleRedemptionRequest>) -> Result<Response<pb::Redemption>, Status> {
		require_permission(&self.state, &request, Permission::RedemptionSettle).await?;
		let id = parse_redemption_id(&request.get_ref().redemption_id)?;
		let redemption = funds_app::settle_redemption(
			self.state.redemptions.as_ref(),
			self.state.nav.as_ref(),
			self.state.ledger.as_ref(),
			&self.state.relay_notify,
			id,
			unix_now(),
		)
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

	async fn list_parked_events(&self, request: Request<pb::ListParkedEventsRequest>) -> Result<Response<pb::ParkedEventList>, Status> {
		require_permission(&self.state, &request, Permission::TreasuryRead).await?;
		let rows = crate::infrastructure::outbox::parked_rows(&self.state.pool)
			.await
			.map_err(|_| Status::unavailable("internal error"))?;
		Ok(Response::new(pb::ParkedEventList {
			events: rows
				.into_iter()
				.map(|r| pb::ParkedEvent {
					seq: r.seq,
					event_id: r.event_id.to_string(),
					aggregate: r.aggregate,
					aggregate_id: r.aggregate_id.to_string(),
					kind: r.kind,
					reason: r.last_error.unwrap_or_default(),
					parked_at: r.parked_at_unix,
					compensated: r.compensated,
				})
				.collect(),
		}))
	}

	async fn unpark_event(&self, request: Request<pb::UnparkEventRequest>) -> Result<Response<pb::UnparkEventResponse>, Status> {
		require_permission(&self.state, &request, Permission::OutboxManage).await?;
		let seq = request.get_ref().seq;
		let unparked = crate::infrastructure::outbox::unpark(&self.state.pool, seq)
			.await
			.map_err(|_| Status::unavailable("internal error"))?;
		if unparked {
			self.state.relay_notify.notify_one();
			return Ok(Response::new(pb::UnparkEventResponse {}));
		}
		// Refused — answer precisely: a compensated park is half-applied with compensation
		// OWED (the relay stamps it at park time, before any recovery runs), so re-driving
		// would re-apply the legs that already posted; a dispatched row has nothing to
		// re-drive.
		match crate::infrastructure::outbox::unpark_refusal(&self.state.pool, seq)
			.await
			.map_err(|_| Status::unavailable("internal error"))?
		{
			Some((_, true)) => Err(Status::failed_precondition("event parked half-applied — compensation is owed; unparking would double-apply")),
			Some((true, _)) => Err(Status::failed_precondition("event already dispatched")),
			_ => Err(Status::not_found("parked event")),
		}
	}

	async fn list_withdrawal_queue(&self, request: Request<pb::ListWithdrawalQueueRequest>) -> Result<Response<pb::WithdrawalQueue>, Status> {
		require_permission(&self.state, &request, Permission::WithdrawalSettle).await?;
		let queued = self.state.withdrawals.list_actionable().await.map_err(map_err)?;
		Ok(Response::new(pb::WithdrawalQueue {
			items: queued
				.into_iter()
				.map(|w| pb::WithdrawalQueueItem {
					withdrawal_id: w.id.to_string(),
					source: if w.source.is_revenue() { "revenue".to_owned() } else { "user".to_owned() },
					// Empty for a revenue payout — the fund owns it, no user does.
					user_id: w.source.user().map(|u| u.to_string()).unwrap_or_default(),
					email: w.email,
					network: w.network.as_str().to_owned(),
					address: w.address,
					amount: w.amount.to_decimal_string(),
					net_amount: w.net_amount.to_decimal_string(),
					state: w.state,
					created_at: w.created_at,
				})
				.collect(),
		}))
	}

	/// The `fee` allocation, on the wire the retired payout view still has (#245): its
	/// cash as `earned`, the reservations as `pending_payout`, and NO rails — nothing pays
	/// this claim out on-chain any more. The supply, the price and the holders wait for
	/// the contract step (C-7) to have a field.
	async fn get_fund_revenue(&self, request: Request<pb::GetFundRevenueRequest>) -> Result<Response<pb::FundRevenue>, Status> {
		require_permission(&self.state, &request, Permission::RevenuePayout).await?;
		let fee = balance_app::fee_allocation(
			self.state.allocations.as_ref(),
			self.state.ledger.as_ref(),
			self.state.nav.as_ref(),
			self.state.issuances.as_ref(),
		)
		.await
		.map_err(map_err)?;
		Ok(Response::new(pb::FundRevenue {
			earned: fee.cash.to_decimal_string(),
			available: fee.available.to_decimal_string(),
			pending_payout: fee.reserved.to_decimal_string(),
			rails: Vec::new(),
		}))
	}

	async fn request_revenue_payout(&self, request: Request<pb::RequestRevenuePayoutRequest>) -> Result<Response<pb::Withdrawal>, Status> {
		// CLOSED ON PURPOSE. This RPC used to pay the fund's revenue out on one Admin/Owner's
		// say-so, gated on the very permission that merely lets someone OPEN a consilium. That
		// made the whole governance mechanism decorative: any principal who could propose a
		// payout could equally well skip the proposal and take the money.
		//
		// The permission check stays FIRST so the refusal reads the same to everyone who could
		// once call this, and tells nobody else that the path exists at all.
		//
		// Since #245 the payout kind itself is retired: the fund's earnings are the `fee`
		// allocation's, held by people, and cash leaves it only by a holder's redemption
		// onto their own claim. `consilium_app::execute` still carries the consilia that
		// were open when the kind was retired, with a withdrawal id DERIVED from the
		// consilium — so the payout path itself carries the proof of authorization rather
		// than trusting its caller.
		require_permission(&self.state, &request, Permission::RevenuePayout).await?;
		let req = request.into_inner();
		tracing::warn!(
			network = %req.network,
			address = %req.address,
			amount = %req.amount,
			"refused a direct fund revenue payout: the fee allocation pays its holders by redemption"
		);
		Err(Status::failed_precondition(
			"the revenue payout is retired: the fund's earnings are held through the fee allocation, and a holder is paid by redeeming their units",
		))
	}

	async fn cancel_revenue_payout(&self, request: Request<pb::CancelRevenuePayoutRequest>) -> Result<Response<pb::Withdrawal>, Status> {
		require_permission(&self.state, &request, Permission::RevenuePayout).await?;
		let id = parse_withdrawal_id(&request.get_ref().withdrawal_id)?;
		let payout = withdrawal_app::cancel_revenue_payout(self.state.withdrawals.as_ref(), &self.state.relay_notify, id)
			.await
			.map_err(map_err)?;
		Ok(Response::new(withdrawal_to_proto(&payout)))
	}

	async fn list_revenue_payouts(&self, request: Request<pb::ListRevenuePayoutsRequest>) -> Result<Response<pb::WithdrawalList>, Status> {
		require_permission(&self.state, &request, Permission::RevenuePayout).await?;
		let payouts = withdrawal_app::list_revenue_payouts(self.state.withdrawals.as_ref()).await.map_err(map_err)?;
		Ok(Response::new(pb::WithdrawalList {
			withdrawals: payouts.iter().map(withdrawal_to_proto).collect(),
		}))
	}

	async fn rotate_deposit_address(&self, request: Request<pb::RotateDepositAddressRequest>) -> Result<Response<pb::RotateDepositAddressResponse>, Status> {
		require_permission(&self.state, &request, Permission::DepositAddressRotate).await?;
		let req = request.get_ref();
		let user = parse_user_id(&req.user_id)?;
		let network = Network::parse(&req.network).map_err(map_err)?;
		let address = self.state.deposit_addresses.rotate(user, network).await.map_err(map_err)?;
		// WARN on success on purpose: a rotation is a recovery event worth an audit trail —
		// the old address is permanently unspendable and users may still hold it.
		tracing::warn!(user_id = %req.user_id, network = %req.network, new_address = %address.as_str(), "rotated a dead deposit address");
		Ok(Response::new(pb::RotateDepositAddressResponse {
			address: address.as_str().to_owned(),
		}))
	}

	/// Phase 4 of the Turnkey migration, one `(user, network)` at a time.
	///
	/// Its own permission, not `DepositAddressRotate`: rotation is emergency recovery for a key
	/// that already cannot sign, this deliberately retires one that can. They share a shape and
	/// nothing else, and whoever may do the first should not thereby be able to do the second.
	async fn migrate_deposit_address_to_custodian(
		&self,
		request: Request<pb::MigrateDepositAddressToCustodianRequest>,
	) -> Result<Response<pb::MigrateDepositAddressToCustodianResponse>, Status> {
		require_permission(&self.state, &request, Permission::DepositAddressMigrate).await?;
		let req = request.get_ref();
		let user = parse_user_id(&req.user_id)?;
		let network = Network::parse(&req.network).map_err(map_err)?;
		let migrated = wallet_app::migrate_deposit_address_to_custodian(
			self.state.deposits.as_ref(),
			self.state.deposit_addresses.as_ref(),
			&self.state.configured_networks,
			user,
			network,
		)
		.await
		.map_err(map_err)?;
		// WARN on success, like a rotation: an address changing hands is an audit event. Both
		// addresses go in the line — the old one is still worth watching, because its key is
		// archived rather than destroyed and a late arrival on it stays recoverable.
		tracing::warn!(
			user_id = %req.user_id,
			network = %req.network,
			old_address = %migrated.old_address,
			new_address = %migrated.new_address.as_str(),
			"migrated a deposit address onto the key custodian — the old address is no longer served"
		);
		Ok(Response::new(pb::MigrateDepositAddressToCustodianResponse {
			old_address: migrated.old_address,
			new_address: migrated.new_address.as_str().to_owned(),
		}))
	}
}

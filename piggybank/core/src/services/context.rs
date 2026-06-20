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
	money::{Network, TxRef, Usdt, WalletAddress},
	users::UserId,
	withdrawals::{Withdrawal, WithdrawalId},
};
use evbanking_auth::claims_of;
use evbanking_contracts::banking::v1::{
	self as pb, allocations_service_server::AllocationsService, balance_service_server::BalanceService, users_service_server::UsersService, wallet_service_server::WalletService,
};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{
	AppState,
	application::{allocations as alloc_app, balance as balance_app, wallet as wallet_app, withdrawals as withdrawal_app},
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
		// The user's single unified claim, read live from TigerBeetle (Read-First);
		// credit-normal, so non-negative.
		let balance = self
			.state
			.ledger
			.balance(&LedgerAccountKey::UserClaim(id))
			.await
			.map_err(|_| Status::unavailable("ledger unavailable"))?;
		Ok(Response::new(pb::UserBalanceResponse {
			amount: balance.posted.to_decimal_string(),
			pending: balance.pending.to_decimal_string(),
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
	async fn get_treasury(&self, request: Request<pb::GetTreasuryRequest>) -> Result<Response<pb::Treasury>, Status> {
		require_admin(&self.state, &request)?;
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

	async fn dispatch_withdrawal(&self, request: Request<pb::DispatchWithdrawalRequest>) -> Result<Response<pb::DispatchWithdrawalResponse>, Status> {
		require_admin(&self.state, &request)?;
		let id = parse_withdrawal_id(&request.get_ref().withdrawal_id)?;
		withdrawal_app::dispatch_withdrawal(self.state.withdrawals.as_ref(), &self.state.relay_notify, id)
			.await
			.map_err(map_err)?;
		Ok(Response::new(pb::DispatchWithdrawalResponse {}))
	}

	async fn settle_withdrawal(&self, request: Request<pb::SettleWithdrawalRequest>) -> Result<Response<pb::SettleWithdrawalResponse>, Status> {
		require_admin(&self.state, &request)?;
		let req = request.into_inner();
		let id = parse_withdrawal_id(&req.withdrawal_id)?;
		let tx_ref = TxRef::parse(&req.tx_ref).map_err(map_err)?;
		withdrawal_app::settle_withdrawal(self.state.withdrawals.as_ref(), &self.state.relay_notify, id, tx_ref)
			.await
			.map_err(map_err)?;
		Ok(Response::new(pb::SettleWithdrawalResponse {}))
	}

	async fn fail_withdrawal(&self, request: Request<pb::FailWithdrawalRequest>) -> Result<Response<pb::FailWithdrawalResponse>, Status> {
		require_admin(&self.state, &request)?;
		let id = parse_withdrawal_id(&request.get_ref().withdrawal_id)?;
		withdrawal_app::fail_withdrawal(self.state.withdrawals.as_ref(), &self.state.relay_notify, id)
			.await
			.map_err(map_err)?;
		Ok(Response::new(pb::FailWithdrawalResponse {}))
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
		let amount = Usdt::parse_decimal(&req.amount).map_err(map_err)?;
		let allocation = alloc_app::allocate_user_stake(self.state.allocations.as_ref(), self.state.ledger.as_ref(), &self.state.relay_notify, user, service, amount)
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

/// `wallet` context — a user's own crypto wallet (balances, deposit addresses,
/// withdrawals). Every RPC acts on the caller's own access-token `sub`.
#[derive(Clone)]
pub struct WalletSvc {
	pub state: AppState,
}

impl WalletSvc {
	pub fn new(state: AppState) -> Self {
		Self { state }
	}
}

#[tonic::async_trait]
impl WalletService for WalletSvc {
	async fn get_wallet(&self, request: Request<pb::GetWalletRequest>) -> Result<Response<pb::Wallet>, Status> {
		let user = caller_id(&request)?;
		let wallet = wallet_app::get_wallet(
			self.state.ledger.as_ref(),
			self.state.allocations.as_ref(),
			self.state.withdrawals.as_ref(),
			self.state.deposit_addresses.as_ref(),
			user,
		)
		.await
		.map_err(map_err)?;
		Ok(Response::new(pb::Wallet {
			balance: Some(pb::Balance {
				available: wallet.balance.available.to_decimal_string(),
				invested: wallet.balance.invested.to_decimal_string(),
				pending_withdrawal: wallet.balance.pending_withdrawal.to_decimal_string(),
				total: wallet.balance.total.to_decimal_string(),
			}),
			deposit_addresses: wallet.deposit_addresses.iter().map(deposit_rail_to_proto).collect(),
			withdrawable: wallet.withdrawable.iter().map(withdrawable_to_proto).collect(),
		}))
	}

	async fn get_deposit_address(&self, request: Request<pb::GetDepositAddressRequest>) -> Result<Response<pb::DepositAddress>, Status> {
		let user = caller_id(&request)?;
		let network = Network::parse(&request.get_ref().network).map_err(map_err)?;
		let address = wallet_app::get_deposit_address(self.state.deposit_addresses.as_ref(), user, network).await.map_err(map_err)?;
		Ok(Response::new(pb::DepositAddress {
			network: network.as_str().to_owned(),
			address: address.as_str().to_owned(),
			min_confirmations: min_confirmations(network),
		}))
	}

	async fn request_withdrawal(&self, request: Request<pb::RequestWithdrawalRequest>) -> Result<Response<pb::Withdrawal>, Status> {
		let user = caller_id(&request)?;
		let req = request.into_inner();
		let network = Network::parse(&req.network).map_err(map_err)?;
		let address = WalletAddress::parse(network, &req.address).map_err(map_err)?;
		let amount = Usdt::parse_decimal(&req.amount).map_err(map_err)?;
		let withdrawal = withdrawal_app::request_withdrawal(
			self.state.withdrawals.as_ref(),
			self.state.ledger.as_ref(),
			self.state.users.as_ref(),
			&self.state.relay_notify,
			user,
			network,
			address,
			amount,
		)
		.await
		.map_err(map_err)?;
		Ok(Response::new(withdrawal_to_proto(&withdrawal)))
	}

	async fn cancel_withdrawal(&self, request: Request<pb::CancelWithdrawalRequest>) -> Result<Response<pb::Withdrawal>, Status> {
		let user = caller_id(&request)?;
		let id = parse_withdrawal_id(&request.get_ref().withdrawal_id)?;
		let withdrawal = withdrawal_app::cancel_withdrawal(self.state.withdrawals.as_ref(), &self.state.relay_notify, id, user)
			.await
			.map_err(map_err)?;
		Ok(Response::new(withdrawal_to_proto(&withdrawal)))
	}

	async fn list_withdrawals(&self, request: Request<pb::ListWithdrawalsRequest>) -> Result<Response<pb::WithdrawalList>, Status> {
		let user = caller_id(&request)?;
		let withdrawals = withdrawal_app::list_withdrawals(self.state.withdrawals.as_ref(), user).await.map_err(map_err)?;
		Ok(Response::new(pb::WithdrawalList {
			withdrawals: withdrawals.iter().map(withdrawal_to_proto).collect(),
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

fn deposit_rail_to_proto(rail: &wallet_app::DepositRail) -> pb::DepositAddress {
	pb::DepositAddress {
		network: rail.network.as_str().to_owned(),
		address: rail.address.as_ref().map(|address| address.as_str().to_owned()).unwrap_or_default(),
		min_confirmations: min_confirmations(rail.network),
	}
}

fn withdrawable_to_proto(rail: &wallet_app::NetworkWithdrawable) -> pb::NetworkWithdrawable {
	pb::NetworkWithdrawable {
		network: rail.network.as_str().to_owned(),
		withdrawable: rail.withdrawable.to_decimal_string(),
		instant: rail.instant.to_decimal_string(),
		min_withdrawal: rail.min_withdrawal.to_decimal_string(),
		withdrawal_fee: rail.withdrawal_fee.to_decimal_string(),
	}
}

fn withdrawal_to_proto(withdrawal: &Withdrawal) -> pb::Withdrawal {
	pb::Withdrawal {
		id: withdrawal.id().to_string(),
		network: withdrawal.network().as_str().to_owned(),
		address: withdrawal.address().as_str().to_owned(),
		amount: withdrawal.amount().to_decimal_string(),
		fee: withdrawal.fee().to_decimal_string(),
		net_amount: withdrawal.net_amount().to_decimal_string(),
		state: withdrawal.state().as_str().to_owned(),
		tx_ref: withdrawal.tx_ref().map(|tx_ref| tx_ref.as_str().to_owned()).unwrap_or_default(),
	}
}

/// Confirmations a watcher waits for before crediting/settling on a network
/// (reorg-safety): BEP20 ~15, TRC20 ~19 (SR rounds), TON a few. Placeholder values.
fn min_confirmations(network: Network) -> u32 {
	match network {
		Network::Bep20 => 15,
		Network::Trc20 => 19,
		Network::Ton => 16,
	}
}

fn parse_withdrawal_id(raw: &str) -> Result<WithdrawalId, Status> {
	Uuid::parse_str(raw).map(WithdrawalId::from_raw).map_err(|_| Status::invalid_argument("invalid withdrawal id"))
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

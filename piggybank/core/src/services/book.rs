//! `book` context — holders trading an allocation's units with each other.
//!
//! Placing acts on the caller's own `sub` behind the same money gates as a subscription
//! (`unfrozen_caller`: the read-only kill-switch and the cross-plane freeze); cancelling
//! is behind `caller_id` only, like cancelling a redemption — a frozen user may still
//! unwind. Every read resolves the allocation as the caller sees it, so the book of a
//! product hidden from them is `NOT_FOUND` exactly as the product is. The policy is an
//! operator's (`Permission::AllocationManage`), the same seam that opens a product.
//!
//! `WatchBook` is the one server stream on the hub. It is a `watch` receiver turned
//! into a stream by hand ([`BookWatch`]): every frame is built by re-reading the book,
//! so a subscriber that lags sees the latest state, never a backlog.
//!
//! `Result<_, Status>` is tonic's mandated handler signature; `Status` is a large
//! type we don't control, so the large-err lint does not apply in this module.
#![allow(clippy::result_large_err)]

use std::{
	future::Future,
	pin::Pin,
	task::{Context, Poll},
};

use domain::{
	authz::Permission,
	balance::ServiceId,
	book::{BookPolicy, CandleResolution, ClientOrderId, OrderId, OrderKind, Price, Side, Tif},
	money::Shares,
	users::UserId,
};
use evbanking_contracts::banking::v1::{self as pb, book_service_server::BookService};
use tokio::sync::watch;
use tonic::{Request, Response, Status, codegen::tokio_stream::Stream};
use uuid::Uuid;

use crate::{
	AppState,
	application::book::{self as book_app, BookPorts, BookSnapshotView, PlaceOrderRequest, WatchFrame},
	infrastructure::outflow::PgOutflowPolicy,
	ports::book::{BookLevel, BookPolicyRecord, Candle, OrderRecord, TradeRecord, UserTrade},
	services::support::{caller_id, holds_permission, map_err, optional, require_permission, unfrozen_caller, unix_now},
};

const DEFAULT_LIST_LIMIT: u32 = 100;
const MAX_LIST_LIMIT: u32 = 200;
const DEFAULT_TAPE_LIMIT: u32 = 50;
const DEFAULT_DEPTH: u32 = 20;
const MAX_DEPTH: u32 = 50;
/// How many public trades ride on each `WatchBook` frame.
const WATCH_TRADES: u32 = 20;

#[derive(Clone)]
pub struct BookSvc {
	pub state: AppState,
}

impl BookSvc {
	pub fn new(state: AppState) -> Self {
		Self { state }
	}

	fn ports<'a>(&'a self, outflow: &'a PgOutflowPolicy<'a>) -> BookPorts<'a> {
		BookPorts {
			allocations: self.state.allocations.as_ref(),
			ledger: self.state.ledger.as_ref(),
			nav: self.state.nav.as_ref(),
			store: self.state.book.as_ref(),
			engine: self.state.book_engine.as_ref(),
			outflow,
			relay: &self.state.relay_notify,
			feed: &self.state.book_feed,
		}
	}

	/// The visibility gate every book read runs: the allocation as the caller sees it,
	/// widened for an `AllocationManage` holder.
	async fn visible<T>(&self, request: &Request<T>, service: &ServiceId) -> Result<UserId, Status> {
		let caller = caller_id(request)?;
		let unrestricted = holds_permission(&self.state, request, Permission::AllocationManage).await?;
		book_app::require_visible(self.state.allocations.as_ref(), service, caller, unrestricted)
			.await
			.map_err(map_err)?;
		Ok(caller)
	}
}

#[tonic::async_trait]
impl BookService for BookSvc {
	async fn place_order(&self, request: Request<pb::PlaceOrderRequest>) -> Result<Response<pb::Order>, Status> {
		let user = unfrozen_caller(&self.state, &request).await?;
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		let side = Side::parse(&req.side).map_err(map_err)?;
		let kind = OrderKind::parse(&req.kind).map_err(map_err)?;
		// A market order is always immediate-or-cancel, so an unstated time in force on
		// one is that rather than a client bug; a limit order has to say.
		let tif = match (kind, optional(&req.tif)) {
			(OrderKind::Market, None) => Tif::Ioc,
			(_, raw) => Tif::parse(raw.unwrap_or_default()).map_err(map_err)?,
		};
		// Parsed at the boundary, so a malformed figure is an `invalid_argument` about the
		// input rather than a validation error from inside the domain.
		let price = optional(&req.price).map(Price::parse_decimal).transpose().map_err(map_err)?;
		let size = Shares::parse_decimal(&req.size).map_err(map_err)?;
		let client_order_id = ClientOrderId::parse(&req.client_order_id).map_err(map_err)?;
		let outflow = PgOutflowPolicy::new(&self.state.pool);
		let record = book_app::place_order(
			&self.ports(&outflow),
			user,
			PlaceOrderRequest {
				service,
				side,
				kind,
				tif,
				price,
				size,
				client_order_id,
			},
		)
		.await
		.map_err(map_err)?;
		Ok(Response::new(order_to_proto(&record, true)))
	}

	async fn cancel_order(&self, request: Request<pb::CancelOrderRequest>) -> Result<Response<pb::Order>, Status> {
		let user = caller_id(&request)?;
		let id = parse_order_id(&request.get_ref().order_id)?;
		let record = book_app::cancel_order(self.state.book.as_ref(), &self.state.relay_notify, &self.state.book_feed, id, user)
			.await
			.map_err(map_err)?;
		Ok(Response::new(order_to_proto(&record, true)))
	}

	async fn list_open_orders(&self, request: Request<pb::ListOpenOrdersRequest>) -> Result<Response<pb::OrderList>, Status> {
		let user = caller_id(&request)?;
		let service = parse_service_filter(&request.get_ref().service)?;
		let records = book_app::list_open_orders(self.state.book.as_ref(), user, service.as_ref()).await.map_err(map_err)?;
		Ok(Response::new(pb::OrderList {
			orders: records.iter().map(|record| order_to_proto(record, true)).collect(),
		}))
	}

	async fn list_order_history(&self, request: Request<pb::ListOrderHistoryRequest>) -> Result<Response<pb::OrderList>, Status> {
		let user = caller_id(&request)?;
		let req = request.get_ref();
		let service = parse_service_filter(&req.service)?;
		let limit = bounded(req.limit, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT);
		let records = book_app::list_order_history(self.state.book.as_ref(), user, service.as_ref(), limit)
			.await
			.map_err(map_err)?;
		Ok(Response::new(pb::OrderList {
			orders: records.iter().map(|record| order_to_proto(record, true)).collect(),
		}))
	}

	async fn list_user_trades(&self, request: Request<pb::ListUserTradesRequest>) -> Result<Response<pb::TradeList>, Status> {
		let user = caller_id(&request)?;
		let req = request.get_ref();
		let service = parse_service_filter(&req.service)?;
		let limit = bounded(req.limit, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT);
		let trades = book_app::list_user_trades(self.state.book.as_ref(), user, service.as_ref(), limit)
			.await
			.map_err(map_err)?;
		Ok(Response::new(pb::TradeList {
			trades: trades.iter().map(user_trade_to_proto).collect(),
		}))
	}

	async fn get_book(&self, request: Request<pb::GetBookRequest>) -> Result<Response<pb::BookSnapshot>, Status> {
		let service = ServiceId::parse(&request.get_ref().service).map_err(map_err)?;
		self.visible(&request, &service).await?;
		let depth = bounded(request.get_ref().depth, DEFAULT_DEPTH, MAX_DEPTH);
		let view = book_app::snapshot(self.state.book.as_ref(), self.state.nav.as_ref(), &service, depth, unix_now())
			.await
			.map_err(map_err)?;
		Ok(Response::new(snapshot_to_proto(&view)))
	}

	async fn list_trades(&self, request: Request<pb::ListTradesRequest>) -> Result<Response<pb::TradeList>, Status> {
		let service = ServiceId::parse(&request.get_ref().service).map_err(map_err)?;
		self.visible(&request, &service).await?;
		let limit = bounded(request.get_ref().limit, DEFAULT_TAPE_LIMIT, MAX_LIST_LIMIT);
		let trades = book_app::list_trades(self.state.book.as_ref(), &service, limit).await.map_err(map_err)?;
		Ok(Response::new(pb::TradeList {
			trades: trades.iter().map(public_trade_to_proto).collect(),
		}))
	}

	async fn list_candles(&self, request: Request<pb::ListCandlesRequest>) -> Result<Response<pb::CandleList>, Status> {
		let service = ServiceId::parse(&request.get_ref().service).map_err(map_err)?;
		self.visible(&request, &service).await?;
		let req = request.get_ref();
		let resolution = CandleResolution::parse(&req.resolution).map_err(map_err)?;
		let to = if req.to == 0 { unix_now() } else { req.to };
		let candles = book_app::candles(self.state.book.as_ref(), &service, resolution, req.from, to).await.map_err(map_err)?;
		Ok(Response::new(pb::CandleList {
			service: service.to_string(),
			resolution: resolution.as_str().to_owned(),
			candles: candles.iter().map(candle_to_proto).collect(),
		}))
	}

	type WatchBookStream = Pin<Box<dyn Stream<Item = Result<pb::BookEvent, Status>> + Send + 'static>>;

	async fn watch_book(&self, request: Request<pb::WatchBookRequest>) -> Result<Response<Self::WatchBookStream>, Status> {
		let service = ServiceId::parse(&request.get_ref().service).map_err(map_err)?;
		// Authorized once, at the handshake — the same rule the BFF applies to its socket.
		let caller = self.visible(&request, &service).await?;
		let depth = bounded(request.get_ref().depth, DEFAULT_DEPTH, MAX_DEPTH);
		let receiver = self.state.book_feed.subscribe(&service);
		Ok(Response::new(Box::pin(BookWatch::new(WatchState {
			state: self.state.clone(),
			service,
			caller,
			depth,
			receiver,
		}))))
	}

	async fn get_book_policy(&self, request: Request<pb::GetBookPolicyRequest>) -> Result<Response<pb::BookPolicy>, Status> {
		let service = ServiceId::parse(&request.get_ref().service).map_err(map_err)?;
		self.visible(&request, &service).await?;
		let record = book_app::policy(self.state.book.as_ref(), &service).await.map_err(map_err)?;
		Ok(Response::new(policy_to_proto(&record)))
	}

	async fn set_book_policy(&self, request: Request<pb::SetBookPolicyRequest>) -> Result<Response<pb::BookPolicy>, Status> {
		require_permission(&self.state, &request, Permission::AllocationManage).await?;
		let req = request.into_inner();
		let service = ServiceId::parse(&req.service).map_err(map_err)?;
		let price_tick = optional(&req.price_tick).map(Price::parse_decimal).transpose().map_err(map_err)?.unwrap_or(BookPolicy::DEFAULT_PRICE_TICK);
		let lot_size = optional(&req.lot_size).map(Shares::parse_decimal).transpose().map_err(map_err)?.unwrap_or(BookPolicy::DEFAULT_LOT_SIZE);
		let policy = BookPolicy::new(req.book_open, req.taker_fee_bps, price_tick, lot_size, req.market_slippage_bps).map_err(map_err)?;
		let record = book_app::set_policy(self.state.allocations.as_ref(), self.state.book.as_ref(), &service, policy)
			.await
			.map_err(map_err)?;
		Ok(Response::new(policy_to_proto(&record)))
	}
}

/// What one `WatchBook` subscription carries between frames.
struct WatchState {
	state: AppState,
	service: ServiceId,
	caller: UserId,
	depth: u32,
	receiver: watch::Receiver<u64>,
}

type FrameFuture = Pin<Box<dyn Future<Output = Option<(Result<pb::BookEvent, Status>, WatchState)>> + Send>>;

/// A `watch` receiver as a tonic stream: wait for the book to move, frame it, repeat.
/// Hand-rolled rather than `tokio_stream::wrappers::WatchStream` because the tonic
/// re-export of `tokio_stream` carries no `sync` feature and the workspace adds no
/// dependency for one wrapper. The state travels through the in-flight future and back,
/// so the stream holds it exactly once at any time.
struct BookWatch {
	state: Option<WatchState>,
	pending: Option<FrameFuture>,
}

impl BookWatch {
	fn new(state: WatchState) -> Self {
		Self {
			state: Some(state),
			pending: None,
		}
	}
}

/// Wait for a change, then frame. `None` only if the feed itself is gone (the sender
/// lives in `AppState`, so in practice the stream ends when the client hangs up).
async fn next_frame(mut watch: WatchState) -> Option<(Result<pb::BookEvent, Status>, WatchState)> {
	watch.receiver.changed().await.ok()?;
	// Only the fact that it moved matters — the frame re-reads the book, so the revision
	// it carries is at least this one and never behind it.
	watch.receiver.borrow_and_update();
	let frame = book_app::watch_frame(
		watch.state.book.as_ref(),
		watch.state.nav.as_ref(),
		&watch.service,
		watch.caller,
		watch.depth,
		WATCH_TRADES,
		unix_now(),
	)
	.await
	.map(|frame| frame_to_proto(&frame))
	.map_err(map_err);
	Some((frame, watch))
}

impl Stream for BookWatch {
	type Item = Result<pb::BookEvent, Status>;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let this = self.get_mut();
		loop {
			if let Some(pending) = this.pending.as_mut() {
				return match pending.as_mut().poll(cx) {
					Poll::Ready(Some((item, state))) => {
						this.pending = None;
						this.state = Some(state);
						Poll::Ready(Some(item))
					}
					Poll::Ready(None) => {
						this.pending = None;
						Poll::Ready(None)
					}
					Poll::Pending => Poll::Pending,
				};
			}
			let Some(state) = this.state.take() else {
				return Poll::Ready(None);
			};
			this.pending = Some(Box::pin(next_frame(state)));
		}
	}
}

fn parse_order_id(raw: &str) -> Result<OrderId, Status> {
	Uuid::parse_str(raw).map(OrderId::from_raw).map_err(|_| Status::invalid_argument("invalid order id"))
}

fn parse_service_filter(raw: &str) -> Result<Option<ServiceId>, Status> {
	optional(raw).map(ServiceId::parse).transpose().map_err(map_err)
}

/// A `0` on the wire is "the server default"; anything above the cap is the cap.
fn bounded(requested: u32, default: u32, max: u32) -> u32 {
	if requested == 0 { default } else { requested.min(max) }
}

/// `own` decides whether `user_id` is disclosed — only ever on the caller's own orders.
fn order_to_proto(record: &OrderRecord, own: bool) -> pb::Order {
	let order = &record.order;
	pb::Order {
		id: order.id().to_string(),
		service: order.service().to_string(),
		user_id: if own { order.user().to_string() } else { String::new() },
		side: order.side().as_str().to_owned(),
		kind: order.kind().as_str().to_owned(),
		tif: order.tif().as_str().to_owned(),
		price: order.price().to_decimal_string(),
		size: order.size().to_decimal_string(),
		filled: order.filled().to_decimal_string(),
		remaining: order.remaining().to_decimal_string(),
		avg_fill_price: order.average_fill_price().map(|price| price.to_decimal_string()).unwrap_or_default(),
		fee_paid: order.fee_paid().to_decimal_string(),
		state: order.state().as_str().to_owned(),
		reject_reason: order.reject_reason().unwrap_or_default().to_owned(),
		client_order_id: order.client_order_id().as_str().to_owned(),
		created_at: record.created_at,
		updated_at: record.updated_at,
	}
}

/// The public tape row: no parties, no order ids.
fn public_trade_to_proto(record: &TradeRecord) -> pb::Trade {
	let trade = &record.trade;
	pb::Trade {
		id: trade.id.to_string(),
		service: trade.service.to_string(),
		price: trade.price.to_decimal_string(),
		size: trade.size.to_decimal_string(),
		taker_side: trade.taker_side.as_str().to_owned(),
		executed_at: record.executed_at,
		user_side: String::new(),
		order_id: String::new(),
		fee: String::new(),
	}
}

/// The caller's own view of a fill: their side, their order, their fee.
fn user_trade_to_proto(view: &UserTrade) -> pb::Trade {
	pb::Trade {
		user_side: view.side.as_str().to_owned(),
		order_id: view.order_id.to_string(),
		fee: view.fee.to_decimal_string(),
		..public_trade_to_proto(&view.trade)
	}
}

fn level_to_proto(level: &BookLevel) -> pb::BookLevel {
	pb::BookLevel {
		price: level.price.to_decimal_string(),
		size: level.size.to_decimal_string(),
		orders: level.orders,
	}
}

fn snapshot_to_proto(view: &BookSnapshotView) -> pb::BookSnapshot {
	pb::BookSnapshot {
		service: view.service.to_string(),
		revision: view.revision,
		bids: view.bids.iter().map(level_to_proto).collect(),
		asks: view.asks.iter().map(level_to_proto).collect(),
		last_price: view.last.map(|(price, _)| price.to_decimal_string()).unwrap_or_default(),
		last_side: view.last.map(|(_, side)| side.as_str().to_owned()).unwrap_or_default(),
		mid: view.mid.map(|price| price.to_decimal_string()).unwrap_or_default(),
		spread: view.spread.map(|price| price.to_decimal_string()).unwrap_or_default(),
		nav: view.nav.to_decimal_string(),
		volume_24h: view.volume_24h.to_decimal_string(),
		change_24h: view.change_24h_pct.clone().unwrap_or_default(),
		as_of: view.as_of,
	}
}

fn frame_to_proto(frame: &WatchFrame) -> pb::BookEvent {
	pb::BookEvent {
		snapshot: Some(snapshot_to_proto(&frame.snapshot)),
		trades: frame.trades.iter().map(public_trade_to_proto).collect(),
		orders_revision: frame.orders_revision,
	}
}

fn candle_to_proto(candle: &Candle) -> pb::Candle {
	pb::Candle {
		time: candle.time,
		open: candle.open.to_decimal_string(),
		high: candle.high.to_decimal_string(),
		low: candle.low.to_decimal_string(),
		close: candle.close.to_decimal_string(),
		volume: candle.volume.to_decimal_string(),
	}
}

fn policy_to_proto(record: &BookPolicyRecord) -> pb::BookPolicy {
	let policy = &record.policy;
	pb::BookPolicy {
		service: record.service.to_string(),
		book_open: policy.book_open(),
		taker_fee_bps: policy.taker_fee_bps(),
		price_tick: policy.price_tick().to_decimal_string(),
		lot_size: policy.lot_size().to_decimal_string(),
		market_slippage_bps: policy.market_slippage_bps(),
		updated_at: record.updated_at,
	}
}

/// The wire vocabulary in `evbanking_contracts::book` is what consumer repos match on;
/// the domain enums are what the hub stores. Drift between them is a test failure here.
#[cfg(test)]
mod tests {
	use domain::book::OrderState;
	use evbanking_contracts::book::{kind as wire_kind, resolution as wire_resolution, side as wire_side, state as wire_state, tif as wire_tif};

	use super::*;

	#[test]
	fn domain_book_vocabularies_match_the_wire_contract() {
		assert_eq!([Side::Buy.as_str(), Side::Sell.as_str()], wire_side::ALL);
		assert_eq!([OrderKind::Limit.as_str(), OrderKind::Market.as_str()], wire_kind::ALL);
		assert_eq!([Tif::Gtc.as_str(), Tif::Ioc.as_str(), Tif::Alo.as_str()], wire_tif::ALL);
		let states = [OrderState::Open, OrderState::PartiallyFilled, OrderState::Filled, OrderState::Cancelled, OrderState::Rejected];
		assert_eq!(states.map(OrderState::as_str), wire_state::ALL);
		for state in states {
			assert_eq!(state.is_resting(), wire_state::is_resting(state.as_str()), "{state:?}");
		}
		assert_eq!(CandleResolution::ALL.map(CandleResolution::as_str), wire_resolution::ALL);
	}

	#[test]
	fn list_limits_default_and_cap() {
		assert_eq!(bounded(0, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT), DEFAULT_LIST_LIMIT);
		assert_eq!(bounded(7, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT), 7);
		assert_eq!(bounded(10_000, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT), MAX_LIST_LIMIT);
	}
}

//! `withdrawals` bounded context — user-initiated on-chain withdrawals.
//!
//! A withdrawal moves a user's free claim out of the fund and onto an external
//! address. It is the **dangerous direction** (value leaves the system), so it is a
//! two-phase saga mirroring a service reservation: the gross amount is *reserved*
//! against the user's claim the instant the request is recorded (pending TigerBeetle
//! transfers — one for the on-chain net, one for the retained fee), then later
//! **settled** (posted, on N on-chain confirmations) or **failed** (voided,
//! refunding the user in full). The cardinal rule — *never void once the broadcast
//! may have reached the chain* — is the operator/watcher's call at settle-vs-fail
//! time; the aggregate only enforces the legal transitions.
//!
//! Pure and wasm-safe (mirrors [`Allocation`](crate::allocations::Allocation)): ids
//! are minted by the application layer, no clock, no I/O. The relay maps each event
//! to ledger ops (and the custody broadcast); that orchestration lives in the
//! adapter, never on this aggregate.

use ev::architecture::{AggregateRoot, DomainEvent, EmitsEvents, Entity, Id};
use serde::{Deserialize, Serialize};

use crate::{
	error::DomainError,
	money::{Network, TxRef, Usdt, WalletAddress},
	users::UserId,
};

/// A unique withdrawal id (UUID). Minted by the application layer.
pub type WithdrawalId = Id<WithdrawalTag>;
/// Phantom tag making [`WithdrawalId`] a distinct, incompatible identity type.
pub struct WithdrawalTag;

/// Per-network withdrawal policy — the flat network fee the fund retains and the
/// minimum gross a user may withdraw. **Placeholder constants** standing in for a
/// real fee oracle (gas/energy/bandwidth fluctuate per chain); kept here so the rule
/// stays pure and testable. Both are whole-USDT, hence representable at every chain's
/// precision, so the on-chain net is never sub-precision dust.
pub struct WithdrawalPolicy;

impl WithdrawalPolicy {
	/// The flat fee retained by the fund on a withdrawal (1 USDT).
	pub const fn fee(_network: Network) -> Usdt {
		Usdt::from_base_units(1_000_000_000_000_000_000)
	}

	/// The smallest gross withdrawal accepted (10 USDT) — necessarily above the fee,
	/// so the on-chain net is always positive.
	pub const fn minimum(_network: Network) -> Usdt {
		Usdt::from_base_units(10_000_000_000_000_000_000)
	}
}

/// Lifecycle of a withdrawal. `Pending` is the reserved, in-flight state (a TB
/// pending transfer per leg); `Completed`/`Failed` are terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WithdrawalState {
	/// Reserved against the user's claim, broadcast (or about to be), awaiting
	/// on-chain confirmation.
	Pending,
	/// Confirmed on-chain; the reservation was posted (terminal).
	Completed,
	/// Never reached the chain; the reservation was voided and the user refunded
	/// (terminal).
	Failed,
}

impl WithdrawalState {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Pending => "pending",
			Self::Completed => "completed",
			Self::Failed => "failed",
		}
	}

	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"pending" => Ok(Self::Pending),
			"completed" => Ok(Self::Completed),
			"failed" => Ok(Self::Failed),
			other => Err(DomainError::Validation(format!("unknown withdrawal state: {other}"))),
		}
	}
}

/// The withdrawal aggregate — the saga's coordinator. Construct via
/// [`Withdrawal::request`] (raises [`WithdrawalEvent::Requested`]) or
/// [`Withdrawal::rehydrate`] (load from the store, no events). The settle/fail
/// commands each accumulate one event, drained by the repository into the event log
/// + outbox in the same unit of work.
#[derive(Debug, Clone)]
pub struct Withdrawal {
	id: WithdrawalId,
	user: UserId,
	network: Network,
	address: WalletAddress,
	amount: Usdt,
	fee: Usdt,
	state: WithdrawalState,
	tx_ref: Option<TxRef>,
	pending: Vec<WithdrawalEvent>,
}

impl Withdrawal {
	/// Open a withdrawal of `amount` (gross) to `address`. `amount` must clear the
	/// per-network minimum and exceed `fee` (so the net is positive), and the net
	/// must be representable at the chain's precision (no dust leak). Raises
	/// `Requested`; the relay reserves the net + fee legs against the user's claim.
	pub fn request(id: WithdrawalId, user: UserId, network: Network, address: WalletAddress, amount: Usdt, fee: Usdt) -> Result<Self, DomainError> {
		if address.network() != network {
			return Err(DomainError::Validation("withdrawal address is for a different network".into()));
		}
		if amount < WithdrawalPolicy::minimum(network) {
			return Err(DomainError::Validation("amount is below the minimum withdrawal".into()));
		}
		let net = amount
			.checked_sub(fee)
			.filter(|net| !net.is_zero())
			.ok_or_else(|| DomainError::Validation("amount does not cover the network fee".into()))?;
		// Reject a net that can't be expressed at the chain's precision — a truncating
		// withdrawal is a slow leak (the same guard as the custody edge).
		net.to_onchain(network)?;
		let mut withdrawal = Self {
			id,
			user,
			network,
			address: address.clone(),
			amount,
			fee,
			state: WithdrawalState::Pending,
			tx_ref: None,
			pending: Vec::new(),
		};
		withdrawal.pending.push(WithdrawalEvent::Requested {
			withdrawal_id: id,
			user,
			network,
			address,
			amount,
			fee,
		});
		Ok(withdrawal)
	}

	/// Reconstitute from the store. Raises no events.
	#[allow(clippy::too_many_arguments)]
	pub fn rehydrate(id: WithdrawalId, user: UserId, network: Network, address: WalletAddress, amount: Usdt, fee: Usdt, state: WithdrawalState, tx_ref: Option<TxRef>) -> Self {
		Self {
			id,
			user,
			network,
			address,
			amount,
			fee,
			state,
			tx_ref,
			pending: Vec::new(),
		}
	}

	/// Settle a confirmed withdrawal: it has the required on-chain confirmations, so
	/// the saga posts the reserved transfers. Records the chain `tx_ref`. Idempotent
	/// if already completed; a failed withdrawal can never be settled.
	pub fn settle(&mut self, tx_ref: TxRef) -> Result<(), DomainError> {
		if self.state == WithdrawalState::Completed {
			return Ok(());
		}
		if self.state != WithdrawalState::Pending {
			return Err(DomainError::Conflict(format!("withdrawal is {}, not settleable", self.state.as_str())));
		}
		self.state = WithdrawalState::Completed;
		self.tx_ref = Some(tx_ref.clone());
		self.pending.push(WithdrawalEvent::Settled {
			withdrawal_id: self.id,
			user: self.user,
			network: self.network,
			amount: self.amount,
			fee: self.fee,
			tx_ref,
		});
		Ok(())
	}

	/// Fail an unsettled withdrawal: the saga voids the reservation, refunding the
	/// user in full. **Only safe when the broadcast certainly did not reach the
	/// chain** — voiding a landed withdrawal double-pays. Idempotent if already
	/// failed; a completed withdrawal can never be failed.
	pub fn fail(&mut self) -> Result<(), DomainError> {
		if self.state == WithdrawalState::Failed {
			return Ok(());
		}
		if self.state != WithdrawalState::Pending {
			return Err(DomainError::Conflict(format!("withdrawal is {}, not failable", self.state.as_str())));
		}
		self.state = WithdrawalState::Failed;
		self.pending.push(WithdrawalEvent::Failed {
			withdrawal_id: self.id,
			user: self.user,
			network: self.network,
			amount: self.amount,
			fee: self.fee,
		});
		Ok(())
	}

	pub fn id(&self) -> WithdrawalId {
		self.id
	}

	pub fn user(&self) -> UserId {
		self.user
	}

	pub fn network(&self) -> Network {
		self.network
	}

	pub fn address(&self) -> &WalletAddress {
		&self.address
	}

	pub fn amount(&self) -> Usdt {
		self.amount
	}

	pub fn fee(&self) -> Usdt {
		self.fee
	}

	/// The amount actually sent on-chain — gross minus the retained fee.
	pub fn net_amount(&self) -> Usdt {
		self.amount.checked_sub(self.fee).unwrap_or(Usdt::ZERO)
	}

	pub fn state(&self) -> WithdrawalState {
		self.state
	}

	pub fn tx_ref(&self) -> Option<&TxRef> {
		self.tx_ref.as_ref()
	}
}

impl Entity for Withdrawal {
	type Id = WithdrawalId;

	fn id(&self) -> WithdrawalId {
		self.id
	}
}

impl AggregateRoot for Withdrawal {
	const NAME: &'static str = "withdrawal";
}

/// Facts raised by the [`Withdrawal`] aggregate. Each carries the saga-relevant data
/// (user, network, amount, fee, address) so the relay maps an event to its ledger
/// ops and the custody broadcast with no extra read. Internally tagged so the stored
/// JSON is self-describing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WithdrawalEvent {
	Requested {
		withdrawal_id: WithdrawalId,
		user: UserId,
		network: Network,
		address: WalletAddress,
		amount: Usdt,
		fee: Usdt,
	},
	Settled {
		withdrawal_id: WithdrawalId,
		user: UserId,
		network: Network,
		amount: Usdt,
		fee: Usdt,
		tx_ref: TxRef,
	},
	Failed {
		withdrawal_id: WithdrawalId,
		user: UserId,
		network: Network,
		amount: Usdt,
		fee: Usdt,
	},
}

impl DomainEvent for WithdrawalEvent {
	const KIND: &'static str = "withdrawals";
}

impl EmitsEvents for Withdrawal {
	type Event = WithdrawalEvent;

	fn drain_events(&mut self) -> Vec<WithdrawalEvent> {
		core::mem::take(&mut self.pending)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn user() -> UserId {
		UserId::new()
	}

	fn addr(network: Network) -> WalletAddress {
		let raw = match network {
			Network::Bep20 => "0x52908400098527886E0F7030069857D2E4169EE7",
			Network::Trc20 => "TJRabPrwbZy45sbavfcjinPJC18kjpRTv8",
			Network::Ton => "EQCD39VS5jcptHL8vMjEXrzGaRcCVYto7HUn4bpAOg8xqB2N",
		};
		WalletAddress::parse(network, raw).unwrap()
	}

	fn gross(units: &str) -> Usdt {
		Usdt::parse_decimal(units).unwrap()
	}

	fn fee(network: Network) -> Usdt {
		WithdrawalPolicy::fee(network)
	}

	#[test]
	fn request_sets_pending_and_emits_requested() {
		let mut w = Withdrawal::request(WithdrawalId::new(), user(), Network::Trc20, addr(Network::Trc20), gross("100"), fee(Network::Trc20)).unwrap();
		assert_eq!(w.state(), WithdrawalState::Pending);
		assert_eq!(w.net_amount(), gross("99"));
		let events = w.drain_events();
		assert_eq!(events.len(), 1);
		assert!(matches!(events[0], WithdrawalEvent::Requested { .. }));
		assert!(w.drain_events().is_empty());
	}

	#[test]
	fn address_network_must_match() {
		let err = Withdrawal::request(WithdrawalId::new(), user(), Network::Trc20, addr(Network::Bep20), gross("100"), fee(Network::Trc20)).unwrap_err();
		assert!(matches!(err, DomainError::Validation(_)));
	}

	#[test]
	fn amount_below_minimum_is_rejected() {
		let err = Withdrawal::request(WithdrawalId::new(), user(), Network::Ton, addr(Network::Ton), gross("5"), fee(Network::Ton)).unwrap_err();
		assert!(matches!(err, DomainError::Validation(_)));
	}

	#[test]
	fn amount_must_cover_the_fee() {
		// A bespoke fee larger than a (minimum-clearing) amount leaves a non-positive net.
		let err = Withdrawal::request(WithdrawalId::new(), user(), Network::Bep20, addr(Network::Bep20), gross("10"), gross("10")).unwrap_err();
		assert!(matches!(err, DomainError::Validation(_)));
	}

	#[test]
	fn settle_posts_and_is_idempotent() {
		let mut w = Withdrawal::request(WithdrawalId::new(), user(), Network::Bep20, addr(Network::Bep20), gross("50"), fee(Network::Bep20)).unwrap();
		w.drain_events();
		w.settle(TxRef::parse("0xhash").unwrap()).unwrap();
		assert_eq!(w.state(), WithdrawalState::Completed);
		assert!(matches!(w.drain_events()[0], WithdrawalEvent::Settled { .. }));
		// Second settle: no error, no event.
		w.settle(TxRef::parse("0xhash").unwrap()).unwrap();
		assert!(w.drain_events().is_empty());
		// A completed withdrawal can't be failed.
		assert!(w.fail().is_err());
	}

	#[test]
	fn fail_voids_and_is_idempotent() {
		let mut w = Withdrawal::request(WithdrawalId::new(), user(), Network::Trc20, addr(Network::Trc20), gross("50"), fee(Network::Trc20)).unwrap();
		w.drain_events();
		w.fail().unwrap();
		assert_eq!(w.state(), WithdrawalState::Failed);
		assert!(matches!(w.drain_events()[0], WithdrawalEvent::Failed { .. }));
		w.fail().unwrap();
		assert!(w.drain_events().is_empty());
		// A failed withdrawal can't be settled.
		assert!(w.settle(TxRef::parse("0xhash").unwrap()).is_err());
	}

	#[test]
	fn event_round_trips_through_json() {
		let mut w = Withdrawal::request(WithdrawalId::new(), user(), Network::Trc20, addr(Network::Trc20), gross("100"), fee(Network::Trc20)).unwrap();
		let event = w.drain_events().pop().unwrap();
		let json = serde_json::to_string(&event).unwrap();
		let back: WithdrawalEvent = serde_json::from_str(&json).unwrap();
		assert!(matches!(back, WithdrawalEvent::Requested { network: Network::Trc20, .. }));
	}
}

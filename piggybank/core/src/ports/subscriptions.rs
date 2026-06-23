//! Persistence + read port for the [`Subscription`] aggregate.
//!
//! A subscription is an immutable mint record, so the write side is a single `open`
//! (no state transitions). `open` also bumps the user's `fund_positions` projection
//! (cost basis + high-water mark) in the **same** transaction as the subscription row
//! and its `Subscribed` event, so the relay then posts the cash move + unit mint
//! (Write-Last).

use async_trait::async_trait;
use domain::{
	architecture::{Reader, Repository},
	error::DomainError,
	subscriptions::Subscription,
};

#[async_trait]
pub trait SubscriptionRepository: Repository<Aggregate = Subscription> + Reader<Aggregate = Subscription> {
	/// Persist a brand-new subscription, drain its `Subscribed` event, and upsert the
	/// user's `fund_positions` cost basis / high-water mark — atomically.
	async fn open(&self, subscription: &mut Subscription) -> Result<(), DomainError>;
}

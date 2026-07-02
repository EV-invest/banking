//! The deposit-address port — a user's per-network address to receive USDT at.
//!
//! On account-model chains (BEP20/TRC20) a per-user deposit address is the only way
//! to attribute an incoming transfer — a USDT transfer carries no memo — so addresses
//! are HD-derived from the fund's xpub. This port hands the application a **stable**
//! address per (user, network); a stub deterministically derives and caches one until
//! the real key-management/derivation service exists.

use async_trait::async_trait;
use domain::{
	architecture::Gateway,
	error::DomainError,
	money::{Network, WalletAddress},
	users::UserId,
};

/// A [`Gateway`]: the signer/key-management seam is a separate trust domain — it
/// owns its own store and can never enrol in a hub Postgres transaction.
#[async_trait]
pub trait DepositAddresses: Gateway {
	/// The user's **fundable** deposit address on `network`, derived once and reused
	/// (stable across calls so a user always sees the same address). `None` means no
	/// fundable address exists yet — the underlying address is still a placeholder (not
	/// the on-chain image of the key), so the rail is presented as unavailable rather than
	/// surfacing an address that cannot receive funds.
	async fn address(&self, user: UserId, network: Network) -> Result<Option<WalletAddress>, DomainError>;
}

//! The `jetton_wallets` store — the jetton wallet each TON wallet's transfers go through,
//! pinned on first use (migration 0007).
//!
//! A jetton transfer's `our_jetton_wallet` is the internal message's destination and
//! receives `msg_value` in Toncoin whatever sits there. The signer cannot derive it (the hub
//! resolves it through an indexer), so the rule is trust-on-first-use: the first transfer a
//! `(wallet, network)` signs pins the address, every later one must agree with it. The
//! policy owns the verdict ([`crate::policy::check_jetton_wallet_pin`]); this module only
//! learns, re-reads and reports.
//!
//! Why its own transaction rather than the spend ledger's: the pin is the wallet's identity,
//! not a spend. A first transfer refused AFTER pinning (on a window, say) still named the
//! right jetton wallet — every pure check on the request had passed — so keeping the pin is
//! correct, and coupling it to the ledger's advisory lock would serialize unrelated windows.
//! The race between two first transfers is settled by the primary key, not a lock.

use domain::money::Network;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::SignerError;

/// What the store found for a `(wallet, network)` when a transfer named `our_jetton_wallet`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JettonWalletPin {
	/// Nothing was pinned yet; this transfer's address is now the pin.
	Learned,
	/// A pin exists — the raw `0:<hex>` rendering stored — and the caller compares against it.
	Pinned(String),
}

/// The signer's jetton-wallet pins over its own database.
#[derive(Clone)]
pub struct JettonWallets {
	pool: PgPool,
}

impl JettonWallets {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}

	/// Pin `jetton_wallet` — already in its stored rendering, the raw `0:<hex>` form
	/// ([`crate::provision::stored_rendering`]), so the primary key is one address and not one
	/// spelling — for `(wallet_id, network)` if nothing is pinned yet, else return what is.
	/// Two first transfers racing on one wallet both reach the INSERT; the primary key lets
	/// one through, and the other re-reads the winner's row — so exactly one address is ever
	/// learned per key.
	pub async fn learn_or_read(&self, wallet_id: Uuid, network: Network, jetton_wallet: &str) -> Result<JettonWalletPin, SignerError> {
		let inserted = sqlx::query("INSERT INTO jetton_wallets (wallet_id, network, jetton_wallet) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
			.bind(wallet_id)
			.bind(network.as_str())
			.bind(jetton_wallet)
			.execute(&self.pool)
			.await?
			.rows_affected();
		if inserted == 1 {
			return Ok(JettonWalletPin::Learned);
		}
		let pinned: Option<String> = sqlx::query_scalar("SELECT jetton_wallet FROM jetton_wallets WHERE wallet_id = $1 AND network = $2")
			.bind(wallet_id)
			.bind(network.as_str())
			.fetch_optional(&self.pool)
			.await?;
		// The signer never deletes a row, so a conflict with nothing to read back is an
		// operator's concurrent manual DELETE; refusing (not re-inserting) is the safe shape.
		pinned
			.map(JettonWalletPin::Pinned)
			.ok_or_else(|| SignerError::Repository(format!("jetton_wallets row for {wallet_id}/{network} vanished between insert and read")))
	}
}

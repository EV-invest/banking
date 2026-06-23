//! TigerBeetle connection.
//!
//! Holds a single connected `tb::Client` — internally synchronised and safe to
//! share across tasks, so it lives behind an `Arc` in [`AppState`]. This module is
//! just the connected client handle; the domain `Ledger` gateway over it is
//! `TbLedger` (in [`ledger`](super::ledger)), which owns the account/transfer
//! mapping. Reach `client()` only for raw ledger ops.
//!
//! [`AppState`]: crate::AppState

use tigerbeetle as tb;

/// A connected TigerBeetle client.
pub struct TigerBeetle {
	client: tb::Client,
}

impl TigerBeetle {
	/// Connect to a TigerBeetle cluster.
	///
	/// `cluster_id` is the cluster identifier (`0` for single-node dev).
	/// `address` is the replica address in any form `tb::Client::new` accepts
	/// (a bare `"3033"`, a `"127.0.0.1:3033"`, …).
	pub fn connect(cluster_id: u128, address: &str) -> anyhow::Result<Self> {
		let client = tb::Client::new(cluster_id, address).map_err(|status| anyhow::anyhow!("tigerbeetle client init failed: {status:?}"))?;
		Ok(Self { client })
	}

	/// Borrow the underlying client to issue ledger operations.
	pub fn client(&self) -> &tb::Client {
		&self.client
	}
}

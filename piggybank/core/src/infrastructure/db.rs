use domain::money::Network;
use sqlx::postgres::{PgPool, PgPoolOptions};

/// Open a connection pool to Postgres (the control plane) with the sqlx-default size
/// (10). The pool is `Clone` and shared through [`AppState`](crate::AppState); the
/// repositories and event log layer on top. sqlx 0.9 already applies sane
/// `acquire_timeout`/`idle_timeout`/`max_lifetime` defaults, so size is the only knob.
pub async fn connect(database_url: &str) -> color_eyre::Result<PgPool> {
	connect_sized(database_url, 10).await
}

/// Open a Postgres pool with an explicit `max_connections`. The composition root sizes
/// the request-serving pool from config and gives the outbox relay its own small pool, so
/// a burst of read traffic and money dispatch can't exhaust each other's connections.
pub async fn connect_sized(database_url: &str, max_connections: u32) -> color_eyre::Result<PgPool> {
	let pool = PgPoolOptions::new().max_connections(max_connections).connect(database_url).await?;
	Ok(pool)
}

/// Apply pending control-plane migrations (embedded from `piggybank/core/migrations`
/// at build time) on startup; also used by integration tests for a hermetic schema.
/// Idempotent. Author new migration FILES with the sqlx CLI
/// (`sqlx migrate add --source piggybank/core/migrations --sequential <name>`),
/// never by hand; the embedded runner here is interoperable with the CLI (same
/// `_sqlx_migrations` table).
pub async fn migrate(pool: &PgPool) -> color_eyre::Result<()> {
	sqlx::migrate!().run(pool).await?;
	Ok(())
}

/// Bind an EVM rail's persisted state to `chain_id` on first boot, or refuse to boot if a
/// later boot disagrees. See `migrations/0020_rail_chain_identity.sql` for the why: the rail's
/// state (`deposit_scan_cursor` block height, `withdrawal_broadcasts` nonce) is keyed by the
/// flat network string, so re-pointing the endpoint at another chain would silently corrupt it.
/// Compared against the CONFIG, never the node's `eth_chainId` — a correct endpoint flip agrees
/// with the config, so only the persisted value sees it, and a DB-only check keeps an
/// unreachable node from turning a chain outage into a failed hub boot.
pub async fn bind_chain_identity(pool: &PgPool, network: Network, chain_id: u64) -> color_eyre::Result<()> {
	// The no-op DO NOTHING then read-back is race-safe without a transaction: two replicas
	// booting the same config both converge on one row, and the row is immutable once written
	// (there is no rebind path), so there is nothing to serialize.
	sqlx::query("INSERT INTO rail_chain_identity (network, chain_id) VALUES ($1, $2) ON CONFLICT (network) DO NOTHING")
		.bind(network.as_str())
		.bind(chain_id as i64)
		.execute(pool)
		.await?;
	let bound: i64 = sqlx::query_scalar("SELECT chain_id FROM rail_chain_identity WHERE network = $1")
		.bind(network.as_str())
		.fetch_one(pool)
		.await?;
	color_eyre::eyre::ensure!(
		bound as u64 == chain_id,
		"the {network} rail is bound to chain {bound} but is now configured for chain {chain_id}. This \
		 database holds {network} state from chain {bound}: a deposit cursor at that chain's block height and \
		 a treasury nonce seeded from that chain's account (both chains share the treasury address). Re-pointing \
		 a rail at a different chain in place is NOT repairable by deleting rows — the TigerBeetle custody and \
		 the network-agnostic user claims those deposits minted are permanent and spendable on every other rail. \
		 Run the new chain against a fresh database, or restore this rail's chain-id override to {bound}."
	);
	Ok(())
}

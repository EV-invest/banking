//! Postgres adapter for the [`Deposits`] port — the aggregate-less company-money
//! facts (seed capital, on-chain deposits) and their outbox events.
//!
//! Each method opens one transaction (the ACID point): the `deposits` gate row and
//! the outbox event commit together or not at all, so the relay can never move money
//! for an unrecorded fact — nor record a fact whose event was lost.

use async_trait::async_trait;
use domain::{
	architecture::DomainEvent,
	balance::{LedgerEvent, Party},
	error::DomainError,
	money::{Network, TxRef, Usdt},
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{infrastructure::outbox, ports::Deposits};

pub struct PgDeposits {
	pool: PgPool,
}

impl PgDeposits {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

#[async_trait]
impl Deposits for PgDeposits {
	async fn seed_capital(&self, network: Network, amount: Usdt) -> Result<(), DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let aggregate_id = Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("fund:{network}").as_bytes());
		let payload = serde_json::to_string(&LedgerEvent::CapitalSeeded { network, amount }).map_err(|e| DomainError::Repository(e.to_string()))?;
		outbox::insert_event(&mut tx, Uuid::new_v4(), "fund", aggregate_id, LedgerEvent::KIND, &payload, true).await?;
		tx.commit().await.map_err(repo_err)
	}

	async fn record(&self, tx_ref: TxRef, party: Party, network: Network, amount: Usdt) -> Result<bool, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let event_id = Uuid::new_v4();
		let inserted = sqlx::query_scalar::<_, String>(
			"INSERT INTO deposits (tx_ref, party_kind, party_id, network, amount, event_id) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (tx_ref) DO NOTHING RETURNING tx_ref",
		)
		.bind(tx_ref.as_str())
		.bind(party.kind_str())
		.bind(party.id_str())
		.bind(network.as_str())
		.bind(amount.base_units().to_string())
		.bind(event_id)
		.fetch_optional(&mut *tx)
		.await
		.map_err(repo_err)?;
		if inserted.is_none() {
			// Already recorded — drop the tx (no-op) and report idempotent success.
			return Ok(false);
		}
		let deposit_id = Uuid::new_v5(&Uuid::NAMESPACE_OID, tx_ref.as_str().as_bytes());
		let payload = serde_json::to_string(&LedgerEvent::Deposited { party, network, amount }).map_err(|e| DomainError::Repository(e.to_string()))?;
		outbox::insert_event(&mut tx, event_id, "deposit", deposit_id, LedgerEvent::KIND, &payload, true).await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(true)
	}
}

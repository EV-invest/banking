//! Application layer — use cases, split CQRS-style.
//!
//! - **Command handlers** (write side) open one [`UnitOfWork`] (a single Postgres
//!   transaction), mutate aggregates, and drain their `EmitsEvents` into the
//!   event log in that same transaction. Money is moved in TigerBeetle **after**
//!   the Postgres commit (Write-Last), via the outbox relay/sagas (a later slice).
//! - **Query handlers** (read side) depend only on the narrow `Reader` ports and
//!   read Postgres projections; authoritative balances come from TigerBeetle.
//!
//! Both depend on the ports in [`crate::ports`], never on concrete adapters.
//!
//! [`auth_sync`] is the first use case: it drains the in-process `Provisioner`
//! channel from the auth task and upserts the [`User`](domain::users::User)
//! aggregate, keeping the hub identity in sync with the verified Google identity.
//! [`balance`] and [`allocations`] are the money use cases — each notifies the relay
//! after its commit so the ledger move follows promptly.

pub mod allocations;
pub mod auth_sync;
pub mod balance;
pub mod funds;
pub mod wallet;
pub mod withdrawals;

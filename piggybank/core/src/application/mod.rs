//! Application layer — use cases, split CQRS-style.
//!
//! - **Command handlers** (write side) build an aggregate and hand it to a single
//!   repository method; that method is its own atomic unit (see [`crate::ports`]) —
//!   it opens the Postgres transaction, mutates the aggregate, and drains its
//!   `EmitsEvents` into the event log in that same transaction. Money is moved in
//!   TigerBeetle **after** the Postgres commit (Write-Last), via the outbox
//!   relay/sagas. The transaction boundary lives in the adapter, not here: there is
//!   no application-layer `UnitOfWork`, because cross-boundary moves are TB sagas
//!   (money written last), never multi-aggregate Postgres transactions.
//! - **Query handlers** (read side) depend only on the narrow `Reader` ports and
//!   read Postgres projections; authoritative balances come from TigerBeetle.
//!
//! Both depend on the ports in [`crate::ports`], never on concrete adapters.
//!
//! [`auth_sync`] is the first use case: it drains the in-process `Provisioner`
//! channel from the auth task and resolves the user the auth task is minting a
//! money-plane token for (users themselves are mirrored from concierge by the
//! one-way bridge, not provisioned here).
//! [`balance`], [`funds`], and [`withdrawals`] are the money use cases — each notifies
//! the relay after its commit so the ledger move follows promptly.

pub mod auth_sync;
pub mod balance;
pub mod funds;
pub mod wallet;
pub mod withdrawals;

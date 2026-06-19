# Money plane — balance & allocations

How the fund ("piggybank") stores and accounts for its money. Cross-cutting context
is in [`docs/ARCHITECTURE.md`](../../docs/ARCHITECTURE.md); this file is the per-area
reference. **Working with money — read this before touching the ledger.**

## Two stores, never joined in one transaction

- **TigerBeetle = data plane** — authoritative amounts (balances, transfers). Never
  re-bookkept in Postgres.
- **Postgres = control plane** — ids, state, the event log + outbox, projections, and
  the `tb_accounts` UUID→`u128` account-id map. Holds **zero** amounts as numbers it
  reasons about (allocation/deposit amounts are stored as exact base-unit TEXT, never
  summed authoritatively in SQL).

## Units

One canonical internal unit: **18-decimal USDT base units** (`domain::money::Usdt`, a
checked `u128`). On-chain decimals differ (BEP20 = 18, TRC20/TON = 6); the custody
edge ([`Usdt::from_onchain`]/[`to_onchain`]) scales by `10^12` and rejects sub-precision
dust. Amounts cross gRPC and the event log as **strings** (proto3 has no `u128`;
`serde_json` has no `u128`) — lossless and JS-safe.

## Chart of accounts (`domain::balance`)

Every account is **per-network** on one USDT value ledger (`ledger = 1`); the bank
mock is a separate USD ledger (`= 2`). Two partitions:

| Side | Accounts (`code`) | Normal | Non-negative flag |
| --- | --- | --- | --- |
| **Custody** (assets) | `wallet:<net>` (10), `bank` (11) | debit (`debits − credits`) | `CreditsMustNotExceedDebits` |
| **Claims** (equity/liab) | `fund:<net>` (1), `user:<uuid>:<net>` (20), `service:<id>:<net>` (30) | credit (`credits − debits`) | `DebitsMustNotExceedCredits` |

A deposit is one balanced transfer **`Dr wallet:<net> / Cr <claim>`** (textbook Dr Cash
/ Cr customer-deposit) — there is no "external world" account. The flags are set **once
at account create** (immutable in TB) and are the last-line backstop against an
over-spent claim or negative custody.

**Why per-network claims (load-bearing).** A claim is backed by custody on its own
network, so `sum(custody:N) == sum(claims:N)` holds **per network** by construction
(deposit/withdraw/allocate all stay on one network; only the fund bridges, moving
custody *and* its own claim together). A chain-specific withdrawal is therefore always
backed and fails cleanly in the domain — a single fungible claim over per-network
custody would hide a network-specific insolvency that a global `sum==sum` can't see.

## Ownership model (`domain::allocations::Allocation`)

An allocation has exactly one **`owner`** (the "owned" party) and a **`sharers`** list
(the "shared" list); the same allocation is *owned* from the owner's side, *shared*
from each sharer's. A `Party::User` in `sharers` may **revoke iff `owner == Piggybank`**
(the [`UserRevocable`] specification); a service-sharer never can.

| Flow | owner | sharers | ledger |
| --- | --- | --- | --- |
| user stake | Piggybank | [User] | `Dr user / Cr service` |
| service reservation | Piggybank | [Service] | **pending** `Dr fund / Cr service` |
| settled / instant transfer | Service | [Piggybank] | posted `Dr fund / Cr service` |

(User stake + balance read are wired through gRPC; the service reservation/settle/
transfer flows are modelled and tested in the domain + gateway, gRPC wiring pending.)

## Write path (Write-Last, Read-First)

A command opens one Postgres tx (the **only** ACID point), mutates the aggregate under
a row lock, and drains its events to `event_log` + `outbox` in that tx. The
single-worker **relay** ([`infrastructure::relay`]) then drains the outbox in strict
`seq` order and issues the TigerBeetle transfer **after** the commit. Existence/balance
checks read TigerBeetle **first** (it's authoritative). Reservations are two-phase TB
pending transfers (`timeout = 0` — the saga owns the lifecycle, never TB's clock).

### Idempotency (at-least-once delivery)

- The stable **`event_id` UUID** (not the delivery cursor `seq`) is the key. A
  single-transfer event's TB id **is** the event id; a reservation's pending uses an
  **allocation-derived** id (`uuid_v5(allocation_id, "reserve")`) so its settle/cancel
  can recompute the same `pending_id`.
- The gateway treats `Exists | AlreadyPosted | AlreadyVoided` as success; a post racing
  its pending is `Retryable` (can't happen under strict `seq`, but handled).
  `InsufficientFunds`/`Conflict` are **parked** (advanced past with a loud log +
  `last_error`) so one bad event can't wedge the queue — a discrepancy reconciliation
  catches; auto-compensation is a follow-up.
- Deposits are idempotent by the `deposits.tx_ref` **gate** (`ON CONFLICT DO NOTHING` →
  emit the event only if newly inserted), so a re-record never double-credits.
- Amounts are **explicit** in every transfer (never TB balancing flags), so a retry
  moves the exact amount frozen into the event.

## Authorization (defense in depth)

Boundary (gRPC) does the cheap stateless check; the stateful rule lives in the
aggregate, applied under the row lock; the TB non-negative flag is the ledger backstop.

| RPC | Who | Boundary | In-tx invariant |
| --- | --- | --- | --- |
| `GetFundBalance` / `SeedCapital` / `RecordDeposit` | admin | `is_admin` + `is_access` | `tx_ref` gate |
| `Allocate` | the user | `sub == user`, `is_access` | sufficient claim (TB flag backstop) |
| `RevokeAllocation` | the user | `sub == user`, `is_access` | owner is the fund ∧ sole user-sharer ∧ active |
| `ListAllocations` | the user | `sub == user` | — |

## Reconciliation (seam)

TB always wins. A reconciliation job (follow-up) must assert, per network: posted
`sum(custody) == sum(claims)`; `*_pending` vs Postgres `Pending` allocations; and
per-network custody ≥ outstanding withdrawal demand. The `last_error` column on a
parked outbox row is the first place to look when money didn't move.

## Tests

`domain` unit tests cover the money math, address parsing, and the revoke rule;
`piggybank/core/tests/balance_allocations.rs` hits **real** Postgres + TigerBeetle
(deposit idempotency, allocate/revoke round-trip + rule, over-allocation rejection, the
non-negative backstop, transfer-id idempotency). It skips when `DATABASE_URL` is unset
or TB is unreachable. Drive the relay deterministically with `Relay::drain`.

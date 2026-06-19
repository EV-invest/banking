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
| **Claims** (equity/liab) | `fund:<net>` (1), `user:<uuid>:<net>` (20), `service:<id>:<net>` (30), `fee:<net>` (40) | credit (`credits − debits`) | `DebitsMustNotExceedCredits` |

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

## User wallet — deposit & withdraw (`domain::withdrawals`, `WalletService`)

A user's own money lives in their per-network claim (`user:<uuid>:<net>`). The wallet
surface adds the two chain-facing directions on top of the allocation flows; balances
are read live (`GetWallet`: per-network `available` = `posted − locked`, `reserved` =
locked-by-withdrawals, `allocated` = sum of active stakes).

**Deposit (chain → claim).** `GetDepositAddress` hands the user a stable per-(user,
network) address from the [`DepositAddresses`] port — a stub HD-derivation that caches
into `user_deposit_addresses`; the real xpub-derivation service is a follow-up.
Crediting still flows through the admin `RecordDeposit` gate (idempotent by `tx_ref`),
the stand-in for a per-network chain watcher that credits only after N confirmations.

**Withdraw (claim → chain) — the dangerous direction.** A [`Withdrawal`] is a two-phase
saga mirroring a service reservation. `RequestWithdrawal` Read-First checks the user's
**available** balance (`posted − locked`, i.e. excluding funds already reserved by other
in-flight withdrawals) covers the gross, gates on the account being active (the KYC/
freeze seam), then records the aggregate `Pending`. The relay reserves **two** pending
legs (deterministic ids `uuid_v5(withdrawal_id, "withdraw:wallet" | "withdraw:fee")`)
and asks custody to broadcast:

| event | relay ops |
| --- | --- |
| `Requested` | reserve `Dr user / Cr wallet:<net>` (net = gross − fee) · reserve `Dr user / Cr fee:<net>` (fee) · then `Custody::broadcast` the net on-chain |
| `Settled` (N confs) | **post** both pending legs — net leaves custody, the fee is retained as revenue |
| `Failed` (never landed) | **void** both pending legs — the user is refunded in full |

`SettleWithdrawal`/`FailWithdrawal` are operator/watcher-driven **admin** RPCs on
`BalanceService` (the stand-in for the watcher + custody confirmation callback). The
cardinal rule — **never `Fail`/void once the broadcast may have reached the chain**
(that double-pays) — is enforced socially at that seam, not by the aggregate. The fee
leg is omitted when the fee is zero (TB rejects a zero-amount transfer); the policy
enforces `min_withdrawal > fee` and the net must be representable at the chain's
precision (no sub-precision dust leaves). The invariant still holds: a withdrawal moves
`gross` out of the user claim, `net` out of custody, and `fee` into the `fee:<net>`
claim, so `sum(custody:N)` falls by exactly `net` and `sum(claims:N)` by `net` too.

**Custody is a separate trust domain.** The [`Custody`] port is the hub's only ask of
the signing service ("broadcast this *already-reserved* withdrawal, idempotently by
id"); the hub never holds keys. A [`StubCustody`] no-op stands in until the real
MPC/HSM service exists — the saga, the ledger, and the RPCs are complete and unchanged
when it lands.

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
- A withdrawal's two pending legs get **withdrawal-derived** ids
  (`uuid_v5(withdrawal_id, salt)`) so settle/fail recompute the same `pending_id`; its
  settle/void completions use distinct salts; and `Custody::broadcast` must be
  idempotent by `withdrawal_id` so an at-least-once relay retry never double-sends.
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
| `GetWallet` / `GetDepositAddress` / `ListWithdrawals` | the user | `sub == user` | — |
| `RequestWithdrawal` | the user | `sub == user`, `is_access` | active account ∧ available claim ≥ gross (TB flag backstop) |
| `SettleWithdrawal` / `FailWithdrawal` | admin | `is_admin` + `is_access` | state is `pending` (idempotent) |

## Reconciliation (seam)

TB always wins. A reconciliation job (follow-up) must assert, per network: posted
`sum(custody) == sum(claims)`; `*_pending` vs Postgres `Pending` allocations; and
per-network custody ≥ outstanding withdrawal demand. The `last_error` column on a
parked outbox row is the first place to look when money didn't move.

## Tests

`domain` unit tests cover the money math, address parsing, the revoke rule, and the
withdrawal aggregate's transitions; `piggybank/core/tests/balance_allocations.rs` and
`piggybank/core/tests/wallet_withdrawals.rs` hit **real** Postgres + TigerBeetle
(deposit idempotency, allocate/revoke round-trip + rule, over-allocation rejection, the
non-negative backstop, transfer-id idempotency; and the withdrawal reserve→settle with
fee retained, fail→full refund, min/available/disabled gates, deposit-address
stability). They skip when `DATABASE_URL` is unset or TB is unreachable. Drive the
relay deterministically with `Relay::drain`.

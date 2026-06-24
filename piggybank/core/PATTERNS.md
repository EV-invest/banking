# Money plane — balance, fund shares & withdrawals

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

**Two layers** on one USDT value ledger (`ledger = 1`); the bank mock is a separate USD
ledger (`= 2`):

| Layer | Accounts (`code`) | Normal | Non-negative flag | Network? |
| --- | --- | --- | --- | --- |
| **Treasury / custody** (assets) | `wallet:<net>` (10), `bank` (11) | debit (`debits − credits`) | `CreditsMustNotExceedDebits` | **per-rail** |
| **Claims** (equity/liab) | `fund` (1), `user:<uuid>` (20), `service:<id>` (30), `fee` (40), `clearing` (50) | credit (`credits − debits`) | `DebitsMustNotExceedCredits` | **network-agnostic** |

A deposit is one balanced transfer **`Dr wallet:<net> / Cr <claim>`** (textbook Dr Cash
/ Cr customer-deposit) — there is no "external world" account. The flags are set **once
at account create** (immutable in TB) and are the last-line backstop against an
over-spent claim or negative custody.

A **third ledger** holds the **service currency** — fund units (`Ledger::Share`, `= 3`),
see [Fund shares](#fund-shares--the-service-currency). It is independent: a unit transfer
can never touch a cash account, so the two planes can't imbalance each other.

**Two layers, network only at the edges (load-bearing).** USDT is one fungible pool, so a
user/service/fund/fee has **one** claim, not one per chain — network lives **only** in the
treasury (`wallet:<net>`) and on deposit/withdrawal *transactions*. The invariant is
therefore **global**: `sum(custody) == sum(claims)` (a deposit grows both sides;
allocate/revoke are claim→claim, net-zero on each sum; a withdrawal drops both by `net`).
Per-rail backing is a **treasury** concern, not a ledger one — a withdrawal on a rail short
of liquidity is *accepted and queued* (see below), never refused. `clearing`
(`WithdrawalClearing`) holds a queued/in-flight withdrawal's reserved gross, decoupled from
any rail so acceptance never depends on rail liquidity. *(This supersedes the original
per-network `sum(custody:N)==sum(claims:N)` model: claims were unified once a single
fungible balance — not three — became the product requirement.)*

## Fund shares — the service currency (`domain::subscriptions`, `domain::redemptions`, `FundsService`)

A client invests by **subscribing** cash into a fund (a `ServiceId`) and receiving
**units** of the service currency, priced at **NAV per share**. A holding's value is
`units × NAV`, so profit comes from a rising NAV, **not** from extra units (standard
NAV/unit accounting). This **replaced** the old flat per-service "allocation" stake.

**Units ledger (`Ledger::Share`, `= 3`).** `UserShares(service, user)` (60, debit-normal,
`shares:<svc>:<uuid>`) is a holder's units; `SharesOutstanding(service)` (61, credit-normal,
`shares_outstanding:<svc>`) is the fund's units in circulation. Per-service invariant
`SharesOutstanding(svc) == Σ_user UserShares(svc, user)`, by construction. **Mint**
`Dr UserShares / Cr SharesOutstanding`; **burn** `Dr SharesOutstanding / Cr UserShares`.
A burn that exceeds the holder's minted units is rejected **by TigerBeetle's flag — even as
a pending reserve** (this is the over-redeem backstop; the PG row-lock only serializes).
`Shares`/`Nav` are 18-dp `u128` newtypes; `Shares::from_cash`/`Nav::value`/`Nav::from_aum`
use an overflow-safe 128×128→256 `mul_div` (a naïve `u128` mul overflows at ~340 USDT).

**NAV is derived, not posted.** An operator posts a fund's **AUM**; the handler reads
`units_outstanding` live from TB and stores `NAV = AUM / units_outstanding` in
`fund_valuations` (append-only marks; the latest is the price, **frozen** between marks).
The first subscription bootstraps at **seed NAV 1.0**. Dealing on a frozen mark is
*backward pricing* — guarded by a **staleness** check (`MAX_NAV_AGE_SECS`); the AUM post is
guarded by a **move** check (`MAX_NAV_MOVE_PCT`, override-able) because the AUM input is the
most dangerous seam ("trusted" ≠ "safe"). NAV is a price, never a TB balance. There is
**no cross-ledger invariant** tying units to USDT — units float; cash stays exact.

**Subscribe (cash → units, synchronous).** Read-First on the unified claim + a fresh NAV;
the relay posts two legs, **cash-first**: `Dr user / Cr service` (the cash pools in the
fund), then mint `Dr UserShares / Cr SharesOutstanding`. Cash-first means an insufficient
claim parks before any units mint — never units without cash.

**Redeem (units → cash, accept-and-queue, settle-time priced).** Units are reserved now;
the cash is **priced and paid at settle** (settle-time NAV, so a queue that drains after a
NAV drop doesn't overpay the redeemer). States `Queued → Completed | Cancelled | Failed`.

| event | → state | relay ops |
| --- | --- | --- |
| `Requested` | `Queued` | reserve a **pending burn** `Dr SharesOutstanding / Cr UserShares` (locks the units) |
| `Settled` | `Completed` | **burn-first**: post the pending burn, then pay `Dr service / Cr user` (`units × settle-NAV`) |
| `Failed` / `Cancelled` | `Failed`/`Cancelled` | **void** the pending burn — units returned |

`request_redemption` settles immediately (via a **separate** command — never co-emitting
`Requested`+`Settled`, which would race the reserve) when the fund claim covers the payout,
else leaves it `Queued` for the operator `SettleRedemption` after the fund tops up (a
deposit to its `service:<id>` claim). The settle pre-check guards the **payout's debit**
(`service` claim `available()`); ordering is **burn-first** so a short fund parks before any
leg, and a raced over-redeem (parked reserve) fails the burn-post **before** any cash leaves
— neither half-applies. Cost basis (average cost) is tracked in `fund_positions` for P&L; a
per-investor `high_water_mark` column is reserved (no fee is charged in v1).

## User wallet — deposit & withdraw (`domain::withdrawals`, `WalletService`)

A user's money is **one** network-agnostic claim (`user:<uuid>`). `GetWallet` presents it
segmented by lifecycle — `available` (`posted − locked`), `invested` (the value of the
user's fund positions, `Σ units × current NAV`), `pending_withdrawal` (sum of
queued/in-flight withdrawals), `total` — plus a
per-rail deposit address and a per-rail **withdrawable** view (`instant = min(available,
rail liquidity)`, the accept-and-queue hint — it discloses a rail's liquidity only up to
the user's own balance; bucket/round it if that must stay private).

**Deposit (chain → claim).** `GetDepositAddress` hands the user a stable per-(user,
network) address from the [`DepositAddresses`] port — a stub HD-derivation cached into
`user_deposit_addresses`; the real xpub service is a follow-up. Crediting flows through the
admin `RecordDeposit` gate (idempotent by `tx_ref`), the stand-in for a chain watcher;
`Dr wallet:<net> / Cr user:<uuid>` credits the **unified** claim regardless of rail.

**Withdraw (claim → chain) — the dangerous direction.** A [`Withdrawal`] is a queued saga.
`RequestWithdrawal` Read-First checks the user's **available** unified claim covers the
gross (user solvency) and gates on the account being active (KYC/freeze), then records the
aggregate. It starts **`Queued`**, with the gross reserved as a pending `Dr user:<uuid> /
Cr clearing` — independent of any rail. The chosen rail's liquidity is the **treasury's**
job: if it can cover the net the withdrawal is dispatched immediately, otherwise it stays
`Queued` until the treasury tops the rail up (accept-and-queue).

| event | → state | relay ops |
| --- | --- | --- |
| `Requested` | `Queued` | reserve `Dr user:<uuid> / Cr clearing` (gross) — no rail touched |
| `Dispatched` | `Processing` | `Custody::broadcast` the net on-chain |
| `Settled` (N confs) | `Completed` | **post** the clearing pending, then `Dr clearing / Cr wallet:<net>` (net) + `Dr clearing / Cr fee` (fee) |
| `Failed` (never landed) | `Failed` | **void** the clearing pending — refund in full |
| `Cancelled` (still queued) | `Cancelled` | **void** the clearing pending — refund in full |

`DispatchWithdrawal` (the treasury worker), `SettleWithdrawal` and `FailWithdrawal` are
operator/worker-driven **admin** RPCs on `BalanceService`; `CancelWithdrawal` is the user's
own (a queued withdrawal only). The cardinal rule — **never void once the broadcast may
have reached the chain** (that double-pays) — is why `Fail` is legal only from `Processing`
and `Cancel` only from `Queued`. The fee leg is omitted when the fee is zero (TB rejects a
zero-amount transfer); the policy enforces `min_withdrawal > fee` and the net must be
representable at the chain's precision (no dust leaves). The global invariant holds at
settle: `user` falls by `gross`, `wallet:<net>` by `net`, `fee` rises by `fee`, and
`clearing` nets back to zero — so `sum(custody)` and `sum(claims)` both fall by exactly
`net`.

**Custody is a separate trust domain.** [`Custody`] is the hub's only ask of the signing
service ("broadcast this *already-reserved* withdrawal, idempotently by id"); the hub never
holds keys. [`StubCustody`] no-ops until the real MPC/HSM service exists — the saga, the
ledger, and the RPCs are complete and unchanged when it lands.

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
  `InsufficientFunds`/`Conflict` are **parked** into a distinct `outbox.parked_at`
  terminal state (NOT `dispatched_at`) so the row stays queryable yet is excluded from
  the drain (`WHERE dispatched_at IS NULL AND parked_at IS NULL`) — one bad event can't
  wedge the queue, and nothing is silently dropped. A park *after* an earlier leg of a
  multi-leg event posted is flagged half-applied (`compensated_at`); the
  [`reconciliation`](src/infrastructure/reconciliation.rs) job surfaces every parked row
  for intervention (TB-reversal of the applied legs is still a follow-up).
- Deposits are idempotent by the `deposits.tx_ref` **gate** (`ON CONFLICT DO NOTHING` →
  emit the event only if newly inserted), so a re-record never double-credits.
- A withdrawal's clearing reservation gets a **withdrawal-derived** id
  (`uuid_v5(withdrawal_id, "withdraw:clearing")`) so settle/fail/cancel recompute the same
  `pending_id`; the settle disbursement + fee posts and the fail/cancel void completions use
  distinct salts; and `Custody::broadcast` must be idempotent by `withdrawal_id` so an
  at-least-once relay retry never double-sends.
- Amounts are **explicit** in every transfer (never TB balancing flags), so a retry
  moves the exact amount frozen into the event.

### Relay safety (single-worker guarantees)

- **Atomic settle.** A withdrawal settle is three legs — post the clearing pending, then
  `Dr clearing / Cr wallet:<net>` (the rail-liquidity backstop), then the fee. Applied
  naïvely these can half-commit: if the rail can't cover the net *after* the pending is
  posted, the user is debited the gross with funds stranded in clearing. Because the relay
  is **single-worker and sequential**, the disburse op is **Read-First rail-checked before
  any leg is applied**: a short rail parks the whole settle atomically (nothing applied),
  recoverable by a rail top-up + reconciliation. (Concurrent withdrawals on one rail are
  the realistic trigger — each is dispatched against the same liquidity, since reserves go
  to `clearing`, not the rail.)
- **Bounded retryable.** Infra outages (`Unavailable`) retry **unbounded**; a *retryable
  ledger state* (`PendingTransferNotFound`) retries **bounded** (`MAX_RETRYABLE_ATTEMPTS`)
  then parks — so a completion whose pending can never appear (its reserve was itself
  parked) cannot wedge the single, globally-ordered queue forever.
- **Parked opening reserve.** If a withdrawal's *first* op — the `clearing` reserve —
  parks (e.g. two concurrent same-user requests both pass the optimistic Read-First, but
  the second violates the user-claim non-negative flag), the control-plane row stays
  `Queued` with nothing reserved. Its later cancel/settle then parks (bounded, no wedge),
  and the [`reconciliation`](src/infrastructure/reconciliation.rs) clearing check (ledger
  reserve vs the gross of in-flight withdrawals) catches the mismatch; the
  [`reaper`](src/infrastructure/reaper.rs) auto-cancels the abandoned `queued` row. A real
  custody broadcast of such a withdrawal must still be refused.

## Authorization (defense in depth)

Boundary (gRPC) does the cheap stateless check; the stateful rule lives in the
aggregate, applied under the row lock; the TB non-negative flag is the ledger backstop.

| RPC | Who | Boundary | In-tx invariant |
| --- | --- | --- | --- |
| `GetTreasury` / `SeedCapital` / `RecordDeposit` | admin | `is_admin` + `is_access` | `tx_ref` gate |
| `Subscribe` | the user | `sub == user`, `is_access` | available claim ≥ cash ∧ fresh NAV (TB flag backstop) |
| `Redeem` | the user | `sub == user`, `is_access` | available units ≥ amount ∧ fresh NAV (TB flag backstop) |
| `CancelRedemption` | the user | `sub == user`, `is_access` | owns it ∧ state is `queued` (idempotent) |
| `GetPosition` / `ListPositions` / `ListRedemptions` / `GetFundNav` | the user | `sub == user` | — |
| `GetWallet` / `GetDepositAddress` / `ListWithdrawals` | the user | `sub == user` | — |
| `RequestWithdrawal` | the user | `sub == user`, `is_access` | active account ∧ available claim ≥ gross (TB flag backstop) |
| `CancelWithdrawal` | the user | `sub == user`, `is_access` | owns it ∧ state is `queued` (idempotent) |
| `DispatchWithdrawal` | admin (treasury) | `is_admin` + `is_access` | state is `queued` (idempotent) |
| `SettleWithdrawal` / `FailWithdrawal` | admin | `is_admin` + `is_access` | state is `processing` (idempotent) |
| `PostFundValuation` | admin | `is_admin` + `is_access` | units outstanding > 0 ∧ NAV move ≤ threshold (or override) |
| `SettleRedemption` / `FailRedemption` | admin (treasury) | `is_admin` + `is_access` | state is `queued` (idempotent) |

## Reconciliation + reaper (recovery jobs)

TB always wins; both jobs are **read-mostly** and run as `select!` branches of the
composition root next to the relay, on the relay's dedicated pool.

[`reconciliation`](src/infrastructure/reconciliation.rs) (`Reconciliation::scan`) asserts
and **alerts** (Sentry-shipped `error!`, no auto-write) on: the **global** posted
`sum(custody) == sum(claims)` on the USDT ledger (read straight from TB via
`Ledger::cash_invariant`); `clearing`'s reserved (pending + posted) balance vs the gross of
every `queued`/`processing` withdrawal in Postgres; and a scan of every `outbox.parked_at`
row (with its `last_error` and `compensated_at`). Per-rail custody vs the real on-chain
wallet balance is the treasury's job, surfaced separately. The `last_error` column on a
parked row is the first place to look when money didn't move.

[`reaper`](src/infrastructure/reaper.rs) (`Reaper::sweep`) owns the timeout for abandoned
sagas (TB pendings are `timeout = 0`, so nothing auto-voids). Split by safety per the
cardinal withdrawal rule: a **`processing` withdrawal** past the max age is **alert-only**
(its broadcast may have landed — voiding would double-pay; only a confirmed not-broadcast
signal may fail it); a **`queued` withdrawal** is **auto-cancelled** (never broadcast →
safe full refund); a **`queued` redemption** is **auto-failed** (internal claim→claim →
safe). Max age is 24h (config seam: `Reaper::with_max_age`).

## Tests

`domain` unit tests cover the money + NAV math (incl. the `mul_div` overflow bound and the
share-key ledger sides), the subscription/redemption aggregates, and the withdrawal
transitions; `piggybank/core/tests/balance_allocations.rs` and
`piggybank/core/tests/wallet_withdrawals.rs` hit **real** Postgres + TigerBeetle
(deposit idempotency, the non-negative backstop, transfer-id idempotency; the Share-ledger
mint/burn + **over-redeem reject by the TB flag**; NAV derivation + the fat-finger guard;
subscribe minting at seed + fractional NAV pricing + staleness/Read-First; redeem
**auto-settle when liquid**, **short fund → queue → top-up → settle at settle-NAV (profit)**,
**short settle parks without burning or paying**, cancel returns the units; and the
withdrawal reserve→settle with fee, fail→refund, short-rail queue→dispatch→settle, queued
cancel→refund). `piggybank/core/tests/relay_recovery.rs` proves a parked event lands in the
distinct `parked_at` state (never marked dispatched), stays queryable, and is surfaced by
`Reconciliation::scan`, and that `Reaper::sweep` alerts on a stuck `processing` withdrawal
(never auto-voids it) while auto-cancelling an abandoned `queued` one. They skip when
`DATABASE_URL` is unset or TB is unreachable. Drive the relay deterministically with
`Relay::drain`, and the recovery jobs with `Reconciliation::scan` / `Reaper::sweep`.

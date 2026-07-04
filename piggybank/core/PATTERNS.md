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
claim parks before any units mint — never units without cash. The `fund_positions`
**cost-basis projection is written by the relay**, after the cash leg posts — *not* on the
synchronous `open` path. Were it written at `open` (as it once was), a cash leg that later
parks (a raced over-subscribe) would strand a **phantom position**: cost basis with no units
and no cash debited, fabricating a P&L loss. Writing it relay-side, after the leg lands,
keeps the projection from ever leading the ledger. The relay's add is `cost_basis += cash`,
made idempotent under at-least-once delivery by a per-event `saga_steps` marker (`leg = 100`,
`role = 'subscribe_position'`) committed in the same transaction as the add — a redelivery
re-applies the TB legs (`Exists`) but the marker gates the relative add to exactly once.

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
— neither half-applies. Cost basis (average cost) is tracked in `fund_positions` for P&L,
alongside the position's **remaining units** (set on the subscribe mint, decremented per
settle). At settle the basis is reduced *proportionally* — `cost_basis ← cost_basis ×
(units − redeemed) / units` — dividing by those **projection-tracked** units under the
`fund_positions` row lock (re-taken inside `settle`, not just `open`), **never** a live
TigerBeetle holding: the unit burn is posted by the relay *after* the settle tx commits, so a
TB read lags it and back-to-back settles would each divide by the same gross pre-burn balance
(under-reducing the basis). Tracking units on the projection makes concurrent/back-to-back
settles compound deterministically. A per-investor `high_water_mark` column is reserved (no
fee is charged in v1).

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
job, and the dispatch gate is **`min(TB rail, on-chain treasury)`**: the TB `wallet:<net>`
balance alone over-counts (it includes confirmed deposits still on users' derived
addresses, which the treasury hot wallet cannot spend), so the gate also reads the custody
adapter's real on-chain treasury USDT (`Custody::treasury_liquidity`; `None` = no chain
view = TB-only, the stub rails). If the effective liquidity covers the net the withdrawal
is dispatched immediately, otherwise it stays `Queued` until the rail is topped up
(accept-and-queue). A treasury read **failure degrades to `Queued`, never a refusal** —
acceptance and the clearing reserve must not depend on a flaky chain node.

| event | → state | relay ops |
| --- | --- | --- |
| `Requested` | `Queued` | reserve `Dr user:<uuid> / Cr clearing` (gross) — no rail touched |
| `Dispatched` | `Processing` | `Custody::broadcast` the net on-chain |
| `Settled` (N confs) | `Completed` | **post** the clearing pending, then `Dr clearing / Cr wallet:<net>` (net) + `Dr clearing / Cr fee` (fee) |
| `Failed` (never landed) | `Failed` | **void** the clearing pending — refund in full |
| `Cancelled` (still queued) | `Cancelled` | **void** the clearing pending — refund in full |

The treasury worker is the [`dispatcher`](src/infrastructure/dispatcher.rs) (see
[Recovery jobs](#reconciliation--reaper--dispatcher-recovery-jobs)); `DispatchWithdrawal`
is its manual override — it refuses (leaving the withdrawal `Queued`, still cancellable)
when the rail treasury provably lacks the net on-chain. `SettleWithdrawal` and
`FailWithdrawal` are operator/watcher-driven **admin** RPCs on `BalanceService`;
`CancelWithdrawal` is the user's own (a queued withdrawal only). The cardinal rule —
**never void once the broadcast may have reached the chain** (that double-pays) — is why
`Fail` is legal only from `Processing` and `Cancel` only from `Queued`. The incident
runbook for a stuck/parked withdrawal is
[`docs/RUNBOOK-withdrawals.md`](../../docs/RUNBOOK-withdrawals.md). The fee leg is omitted when the fee is zero (TB rejects a
zero-amount transfer); the policy enforces `min_withdrawal > fee` and the net must be
representable at the chain's precision (no dust leaves). The global invariant holds at
settle: `user` falls by `gross`, `wallet:<net>` by `net`, `fee` rises by `fee`, and
`clearing` nets back to zero — so `sum(custody)` and `sum(claims)` both fall by exactly
`net`.

**Custody is a separate trust domain.** [`Custody`] is the hub's only ask of the signing
service ("broadcast this *already-reserved* withdrawal, idempotently by id"); the hub never
holds keys. [`StubCustody`] no-ops until the real MPC/HSM service exists — the saga, the
ledger, and the RPCs are complete and unchanged when it lands. The **signer** applies its own
spend policy as an independent second gate (holds even if the hub is compromised): a
per-transfer USDT cap (`SIGNER_MAX_TRANSFER_USDT`) and an optional destination allowlist
(`SIGNER_DESTINATION_ALLOWLIST`) on treasury-sourced transfers — both no-ops until configured,
so set the cap before scaling real liquidity.

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
  for intervention (TB-reversal of the applied legs is still a follow-up). Parked rows
  are operator-unparkable once the cause is fixed (`BalanceService.UnparkEvent`:
  `parked_at` cleared **and** `attempts` reset — a retry-exhausted row would otherwise
  re-park on first redelivery — then the relay is notified); a **compensated** row is
  refused, since its recovery event already applied and re-driving would double-apply.
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
- **No shutdown cascade on a DB blip.** The composition root cancels every sibling task when
  any branch returns, so the relay must not exit on a transient DB failure. `Relay::run`
  re-acquires the outbox advisory lock with capped backoff when the lock connection drops (or
  initial acquisition errors) instead of returning — a Postgres hiccup pauses the drain, it
  doesn't tear down the money plane. Cancellation is still observed at every wait point.
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
  custody broadcast of such a withdrawal is **refused**: the `Dispatched` event's broadcast
  op is the one op with no TB leg whose flags could reject it (it is an external side
  effect), so the relay guards it with a Read-First on the reserve having actually applied —
  its `CLEARING_RESERVE` transfer id recorded in `saga_steps` by the strictly-earlier
  Requested leg. No step ⇒ the reserve parked ⇒ the broadcast is parked, never sent. This
  closes the over-withdrawal race where a double-submit (TB lags the committed-but-undrained
  reserve, so the second request passes the optimistic Read-First) would otherwise broadcast
  real money with nothing locked. The `saga_steps` insert is therefore load-bearing, not
  best-effort: a failed insert retries the whole (idempotent) event. Beside it sits the
  **broadcast-state guard**: the withdrawal row must still be `processing`, or the
  `Dispatched` event is parked (never sent). This makes unparking a `Dispatched` event
  *after* the withdrawal was failed/cancelled — a broadcast against a voided reservation,
  the unpark-after-fail double-pay hazard — structurally impossible rather than a runbook
  discipline.
- **On-chain treasury Read-First (custody).** The dispatch gate already min-s the TB
  accounting balance with the adapter's on-chain treasury read (`treasury_liquidity`,
  USDT-only), so an underfunded rail normally queues instead of ever reaching custody. But
  the gate is check-then-act — the on-chain balance can still drop between the dispatch-time
  read and the broadcast (a parallel withdrawal, an out-of-band spend, a gas-only
  shortfall). So each custody adapter **also** Read-Firsts the **real** treasury balance
  (USDT to send + native gas) before it allocates a nonce/seqno or signs — the last-line
  backstop behind the gate; a shortfall **parks** (`Rejected`) rather than retrying, so an
  underfunded rail can't wedge the single-worker drain. That residual park is rare,
  operator-visible (reconciliation), and recovered via
  [`docs/RUNBOOK-withdrawals.md`](../../docs/RUNBOOK-withdrawals.md). On BSC and TON a node
  rejection of a *first-ever* send additionally frees its stored nonce/seqno (`discard_tx`)
  so the sequence never gaps at a slot nothing will fill.
- **Provable death before re-sign (TRON/TON).** A nonce-free rail (TRON, TON) can only
  re-sign a stuck send once it is *provably* dead, never merely past its local-clock
  expiration: TRON waits until the **solidified** head's timestamp is past the tx expiration
  (+ margin) with no receipt; TON re-signs at the **same seqno** with a fresh window (only one
  message per seqno can ever be accepted, and the replaced one is expired) — which also
  unfreezes the strictly-sequential seqno pipeline when a send expired before its turn. A
  wall-clock-only re-sign would double-pay a tx that later lands.
- **Settlement proof, not seqno advance (TON).** A treasury seqno advance only proves the
  wallet processed *an* external message — a bounced jetton transfer advances it too, with the
  USDT returned. The TON watcher therefore settles only on a matching non-aborted **outgoing**
  jetton transfer from the indexer (the mirror of the deposit path), recording that transfer's
  real tx hash; a seqno advance with no such transfer leaves the withdrawal `processing`
  (reserve held, operator/reaper-recoverable) rather than settling a phantom disbursement.
- **Cross-flow claim contention (shared per-user lock).** Withdraw and subscribe both spend
  the **same** `UserClaim`, yet live in different tables — so a per-table `FOR UPDATE` (the
  redemptions' `fund_positions` lock) does **not** serialize a withdraw against a subscribe.
  Both `PgWithdrawals::open` and `PgSubscriptions::open` therefore take one **shared** lock
  first: `pg_advisory_xact_lock` keyed on the user id ([`outbox::lock_user`]), held to commit.
  This serializes the two `open` transactions on a single target (an advisory lock needs no
  `users` row and no FK, so it engages unconditionally). It shrinks but does not erase the
  optimistic-Read-First window: the reservation is applied by the relay **after** commit, so a
  fully race-free read-and-reserve would need a PG-side reserved counter (a deliberate
  follow-up). What the lock + relay **do** guarantee today is no silent divergence — a raced
  over-commit parks (TB's non-negative flag), recoverable via reconciliation, and the
  combined fix never leaves a phantom: a parked subscribe writes **no** cost_basis (see
  Subscribe, above), and a parked withdrawal reserve leaves nothing reserved (above).

## Authorization (defense in depth)

Boundary (gRPC) does the cheap stateless check; the stateful rule lives in the
aggregate, applied under the row lock; the TB non-negative flag is the ledger backstop.

| RPC | Who | Boundary | In-tx invariant |
| --- | --- | --- | --- |
| `GetTreasury` / `SeedCapital` / `RecordDeposit` | operator | `require_permission` (RBAC matrix) | `tx_ref` gate |
| `Subscribe` | the user | `sub == user`, `is_access`, **not frozen** | available claim ≥ cash ∧ fresh NAV (TB flag backstop) |
| `Redeem` | the user | `sub == user`, `is_access`, **not frozen** | available units ≥ amount ∧ fresh NAV (TB flag backstop) |
| `CancelRedemption` | the user | `sub == user`, `is_access` | owns it ∧ state is `queued` (idempotent) |
| `GetPosition` / `ListPositions` / `ListRedemptions` / `GetFundNav` | the user | `sub == user` | — |
| `GetWallet` / `GetDepositAddress` / `ListWithdrawals` | the user | `sub == user` | — |
| `RequestWithdrawal` | the user | `sub == user`, `is_access`, **not frozen** | active account ∧ available claim ≥ gross (TB flag backstop) |
| `CancelWithdrawal` | the user | `sub == user`, `is_access` | owns it ∧ state is `queued` (idempotent) |
| `DispatchWithdrawal` | operator (treasury) | `require_permission` (RBAC matrix) | state is `queued` (idempotent) |
| `SettleWithdrawal` / `FailWithdrawal` | operator | `require_permission` (RBAC matrix) | state is `processing` (idempotent) |
| `PostFundValuation` | operator | `require_permission` (RBAC matrix) | units outstanding > 0 ∧ NAV move ≤ threshold (or override) |
| `SettleRedemption` / `FailRedemption` | operator (treasury) | `require_permission` (RBAC matrix) | state is `queued` (idempotent) |
| `GetUserBalance` | operator | `require_permission` (RBAC matrix); resolves the CONCIERGE id first via the bridge mirror (`users.concierge_user_id`), then the banking id; unknown ⇒ `NOT_FOUND` | — |
| `ListParkedEvents` | operator | `require_permission` (RBAC matrix) | — |
| `UnparkEvent` | admin (`OutboxManage`) | `require_permission` (RBAC matrix) | parked ∧ not dispatched ∧ **not compensated** (the double-apply guard) |

`require_permission` (`services::support`) is `is_access` + the pure RBAC matrix
(`domain::authz::grants` — the single place the matrix is defined) over the caller's
bridge-mirrored role, **after** the account gates: a `disabled` (or frozen) operator is
refused, a stale `token_version` is refused, and the `ADMIN_SUBJECTS` allowlist is only a
break-glass **role override** — it never bypasses those gates when a local row exists.

**Cross-plane freeze gate** (`services::support::unfrozen_caller`): the value-leaving RPCs
above (`Subscribe`/`Redeem`/`RequestWithdrawal`) reject with `failed_precondition` when the
caller's banking row is `frozen`. `frozen` is set by the one-way concierge→banking lifecycle
bridge consumer (`infrastructure::bridge`), which PULLS `UserLifecycleEvent`s from the
concierge plane (`UserEvents.PullUserLifecycle`, `BRIDGE_SERVICE_TOKEN`) and mirrors
SUSPENDED→frozen / REINSTATED→unfrozen, KYC, and the revoke floor onto `users`, keyed by
`auth_subject` and dedup/ordered by per-user `sequence`. Identity stays owned by concierge;
banking only mirrors the gating slice. The gate fails CLOSED (UNAVAILABLE) if the flag can't
be read. Cancel/read RPCs are intentionally NOT gated, so a frozen user can still unwind
queued positions.

## Reconciliation + reaper + dispatcher (recovery jobs)

TB always wins; the jobs run as `join!` branches of the composition root next to the
relay, on the relay's dedicated pool.

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

[`dispatcher`](src/infrastructure/dispatcher.rs) (`Dispatcher::sweep`, every 30s) is the
treasury worker: it re-checks every `queued` withdrawal against **both** liquidity gates —
the TB rail balance and `Custody::treasury_liquidity` — and dispatches the covered ones
(idempotently, via the same row-locked command as the admin RPC), so a rail top-up
self-heals the queue within one interval. A treasury read `Err` skips that cycle (the
automatic path stays conservative; the operator RPC may still exercise judgment). Together
with the reaper this brackets accept-and-queue: dispatched within ~30s of a top-up, or
auto-cancelled (refunded) at 24h — the de-facto rail top-up SLA.

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
cancel→refund; the on-chain dispatch gate's three arms — short treasury queues despite a
liquid TB rail, liquid treasury dispatches, read failure degrades to queued — plus the
refused admin dispatch and the `Dispatcher::sweep` both-gates flow, driven by a test
`Custody` adapter with a configurable treasury view). `piggybank/core/tests/relay_recovery.rs`
proves a parked event lands in the distinct `parked_at` state (never marked dispatched),
stays queryable, and is surfaced by `Reconciliation::scan`; that `Reaper::sweep` alerts on
a stuck `processing` withdrawal (never auto-voids it) while auto-cancelling an abandoned
`queued` one; and that an unparked `Dispatched` event for a failed withdrawal is re-parked
by the broadcast-state guard, never sent. They skip when `DATABASE_URL` is unset or TB is
unreachable. Drive the relay deterministically with `Relay::drain`, and the recovery jobs
with `Reconciliation::scan` / `Reaper::sweep` / `Dispatcher::sweep`.

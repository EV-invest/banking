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
checked `u128`). On-chain decimals differ (BEP20 = 18, TRC20/TON/**Polygon** = 6); the custody
edge ([`Usdt::from_onchain`]/[`to_onchain`]) scales by `10^12` and rejects sub-precision
dust. Amounts cross gRPC and the event log as **strings** (proto3 has no `u128`;
`serde_json` has no `u128`) — lossless and JS-safe.

**Rails.** Two chain families back the same fungible USDT pool. **EVM** — BEP20 (BSC) and
**Polygon (PoS)** — share one parameterized adapter set (`evm_rpc` + `ChainCustody`/`DepositWatcher`/
`WithdrawalWatcher`/`Sweep`, one instance per rail keyed by `EvmConfig::network`): identical
`eth_getLogs`/receipt/nonce/legacy-signing machinery, differing only in chain-id, USDT contract,
token decimals (BEP20 18-dp vs Polygon 6-dp), and native gas coin (BNB vs POL). Each EVM rail's
`withdrawal_broadcasts` nonce sequence is SQL-scoped by `network`, so the two never collide. **Non-EVM**
— TRON (TRC20) and TON (jetton) — keep their own per-rail files (`tron_*`, `ton_*`) because their
curves/tx formats/RPC shapes can't share the EVM path. A rail runs only when its endpoint env is set
(`POLYGON_RPC_URL` for Polygon); its treasury + gas-station are per-network wallets the operator funds
separately (USDT + native coin).

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

## Allocations — the registry of investable products (`domain::allocations`, `AllocationsService`)

An **allocation** is the control-plane record that makes a `ServiceId` investable. It is
the *only* way a fund comes into existence.

**The hole it closed.** `Subscribe` used to validate only the slug's *shape*
(1..64 `[A-Za-z0-9_-]`), and `dealing_nav` bootstraps a service with no valuation at the
**seed NAV 1.0** — so any investor could mint a fund by typing an unregistered id, moving
real money into a `service:<typo>` claim with no operator behind it. `PostFundValuation`
was the second such door. Both now resolve the registry first.

**States.** `draft` (registered, accepts no money, hidden from the default catalog) ->
`open` (subscribe + redeem) -> `closed` (**redeem only**). `closed -> open` re-opens;
`draft` is entered only by registration, so a product that has taken money can never
travel back to "never opened". Transitions are idempotent — re-opening an open allocation
raises no event.

**The asymmetric gate is the point.** `require_subscribable` admits only `open`;
`require_redeemable` admits `open` *and* `closed`. Winding a product down must never trap
an investor's units inside it, so only the *new money* direction is ever blocked.

**Access — closed by default (migration `0033`).** A second axis, orthogonal to the
lifecycle: `state` says whether the product deals, `access` says *with whom*. Each
allocation carries a default level `hidden < view < invest` (`AllocationAccess`, ranked —
the derived `Ord` is the policy), and `allocation_access_grants` raises individual
investors above it (`view` | `invest`, never `hidden`: a grant only adds). The level an
investor effectively holds is `max(default, grant)` — **computed, never stored**: the read
port `LEFT JOIN`s the caller's grant and the domain folds the two. A registration lands on
`view` (listed once open, locked); `0033` backfilled every `open` row to `invest` so the
release changed nothing for live products. `ListAllocations` shows an investor `open`
products at `view` or above; `GetAllocation` answers NOT_FOUND for a `hidden` one (as for
an unregistered slug); `Subscribe` refuses below `invest` with `DomainError::Precondition`
→ FAILED_PRECONDITION, distinguishable from "not open" and the cap (both `Validation`).
`require_redeemable` never consults access — a locked product still lets holders out.
`AllocationManage` reads everything, but `caller_access` stays honest for a manager too:
the permission reads the product, it does not invest in it. Grant/revoke facts land in
`event_log` under the allocation aggregate (`relay = false`, like every registry event).

**Authorization.** Registration and every transition need `Permission::AllocationManage`
— Admin/Owner, alongside `ValuationPost`, never Operator: bringing a fund into existence
is not a read. Reads are open to any authenticated user; `include_unlisted` (drafts +
closed) is behind the same permission and **refuses** rather than silently downgrading to
the open-only list.

**No money, so no relay.** An allocation moves no value: its events are drained with
`relay = false` — audit facts in `event_log`, never in the `outbox` (the relay has no
ledger op for the kind and would park the row). Identity is the `ServiceId` slug; the
table's UUID `id` is a surrogate carried only because `event_log.aggregate_id` is a UUID.

**Wire contract.** `contracts/proto/banking/v1/allocations.proto` plus the hand-written
`evbanking_contracts::allocation` facade, which gathers the stubs and pins the state
vocabulary consumer repos match on. `domain::allocations::AllocationState::as_str` must
stay byte-identical with it (guarded by `allocation_state_strings_are_canonical` and
`domain_states_match_the_wire_contract`).

**Migration `0021`** drops the dead `allocations` table from `0002` (the write store of
the abandoned per-service stake aggregate, unread by any code since `0005`) and backfills
every service already carrying subscriptions/redemptions/positions/valuations as `open` —
without that backfill the gate would retroactively lock existing investors out of
`Redeem`.

## Fund shares — the service currency (`domain::subscriptions`, `domain::redemptions`, `FundsService`)

A client invests by **subscribing** cash into a fund (a `ServiceId`) and receiving
**units** of the service currency, priced at **NAV per share**. A holding's value is
`units × NAV`, so profit comes from a rising NAV, **not** from extra units (standard
NAV/unit accounting). This **replaced** the original flat per-service *stake* aggregate
(confusingly also called an "allocation" then — the name now belongs to the registry
above, which is a different thing entirely: a product's listing, not a holding).

**Units ledger (`Ledger::Share`, `= 3`).** `UserShares(service, user)` (60, debit-normal,
`shares:<svc>:<uuid>`) is a holder's units; `SharesOutstanding(service)` (61, credit-normal,
`shares_outstanding:<svc>`) is the fund's units in circulation. Per-service invariant
`SharesOutstanding(svc) == Σ_user UserShares(svc, user) + Σ_user BookShares(svc, user) + FeeShares(svc) + CompanyShares(svc)`,
by construction (the extra holders are introduced under [Fees](#fees--2-and-20-domainfees-feesservice-feesweeper),
[In-kind issuance](#in-kind-issuance--the-companys-stake-domainissuance-allocationsserviceissueunits) and
[The book](#the-book--holders-trading-units-with-each-other-domainbook-bookservice)). **Mint**
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
guarded by a **move** check (`MAX_NAV_MOVE_PCT`) because the AUM input is the most
dangerous seam ("trusted" ≠ "safe"). NAV is a price, never a TB balance. There is
**no cross-ledger invariant** tying units to USDT — units float; cash stays exact.

**The move check has no override, and it is measured twice** (banking#232). The cap is
checked against the previous mark AND against the mark anchoring a rolling
`NAV_MOVE_WINDOW_SECS` window (the newest mark at least that old, or the fund's first mark
while it is younger than the window) — so +49% seventeen times in an afternoon is refused
on the second post, not compounded into a 900× price. A move the guard refuses is a
**valuation-override consilium** (`docs/CONSILIUM.md` § Valuation override): the poster
proposes, the owners vote, and executing it records the mark through the same writer
(`funds::record_valuation`) with the guard simply not consulted. A subject who posted a
mark for a fund — directly or by proposing the override — **cannot redeem from that fund
for `VALUATION_REDEEM_COOLDOWN_SECS`**, checked at request AND at settle, so the person
who moved the price is never the person cashing out at it. A hard ceiling "AUM ≤ cash in
the fund's claim" was considered and rejected: an in-kind product (§ In-kind issuance) is
worth its asset with zero cash in its claim, and that invariant would make it unmarkable.

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
settles compound deterministically. The reduction applies **exactly once** per redemption: a
repeat `SettleRedemption` (documented idempotent) short-circuits on the already-`Completed`
row before reducing, and a settle that outruns the relay's subscribe projection (the redeem
is admitted off the TB balance, which the projection lags) **rolls back** with `Conflict` —
redemption still `Queued`, settleable by the operator once the projection lands — rather
than silently skip a reduction the projection would then overwrite into a permanently
overstated basis. The auto-settle inside `Redeem` degrades that `Conflict` to re-reading and
returning the redemption's actual state (queued, or a raced terminal) instead of an error. The
per-investor `high_water_mark` column reserved here is now live — see [Fees](#fees--2-and-20-domainfees-feesservice-feesweeper).

## In-kind issuance — the company's stake (`domain::issuance`, `AllocationsService.IssueUnits`)

A subscription is cash-for-units. It has no honest shape for a product registered against
an asset that **already has owners**: `service_arb` is valued at $16 250 with 80 % of the
units the company's and 20 % a named investor's, and nobody wires cash into a fund claim
to make that true. `IssueUnits` (`AllocationManage`) is the second supply path: a mint
**with no cash leg**, to a `UnitHolder` that is a user or **the company itself**, at a
cost basis the operator states (`units × NAV` when they do not — the same figure a
subscription for those units would have cost at the mark). It lives on the registry
service, not `FundsService`, because it is a decision about who holds what, made by the
operator who sizes the product, not an investor dealing at NAV.

**A third unit holder.** `CompanyShares(service)` (63, debit-normal,
`shares_company:<svc>`) is a holder exactly like a user or the fee account, so the
Share-ledger invariant becomes `SharesOutstanding == Σ UserShares + FeeShares +
CompanyShares` and the issued units count against the allocation's cap as any mint does.
The company is a holder in its own right rather than a user with a well-known id: it has no
`users` row, no `fund_positions` projection and no P&L, and a synthetic user would drag
every investor-facing read into special-casing one UUID. `ListUnitHolders` reports the
split — `company_units`, `fee_units`, `investor_units = outstanding − company − fee` —
read straight from TB; `FundNav.company_units` shows an investor the company's share on
the card. The mint posts under its own `TransferCode::UnitIssue` (47), not `ShareMint`,
so supply growth the fund's cash never paid for is distinguishable from a subscription's
on the Share ledger alone.

**Same path as a subscription, minus the cash.** The use case (`application::issuance`)
resolves the allocation (registered in **any** state — a product is normally seeded before
it opens, and a closed one may still need its cap table corrected; access is not consulted,
this is an operator command), requires a user holder to **exist** (units minted to a UUID
nobody can sign in as are units nobody can redeem — the DB reference on
`unit_issuances.holder_id` backs the same rule), prices at the fresh dealing NAV (the mark
is recorded on the row and blended into the holder's high-water mark, so a stale one is
refused as it is for a subscription), and runs `ensure_capacity` against the ledger's
issued supply — an operator sizing a product below what they mean to issue raises the cap
first. The control-plane row (`unit_issuances`, migration `0031`) and the `Issued` event
commit together; the relay posts `Dr <holder shares> / Cr SharesOutstanding` with a
transfer id derived from the **issuance id** (`tid(issuance, "issue:mint")`), then — after
the leg lands, never on the open path — stamps the row `queued → applied` and, for a user
holder, adds the cost basis to `fund_positions` under the same per-event `saga_steps`
marker discipline as the subscribe projection (`leg = 100`, `role = 'issue_applied'`). A
parked mint therefore leaves a `queued` row and no basis, never a row claiming units it
did not get.

**Idempotent by an operator-supplied key**, unique per service (`1..64` chars): the
console generates one per form submission and re-sends the same one on a timeout, so a
double click lands one mint. The use case reads the key **before** pricing (a retry must
succeed after the mark went stale); the adapter's `INSERT … ON CONFLICT DO NOTHING` settles
the concurrent case and hands the loser the winner's row, and the event is drained only
when the insert landed. A repeat asking for the same holder and units returns the row as it
stands now; the same key for a **different** request is `Conflict` → ALREADY_EXISTS — the
retry is what the key exists for, the reuse is the mistake it has to catch.

**The service_arb recipe.** Register → `IssueUnits` 20 % to the investor and 80 % to the
company at the seed NAV with the agreed bases → `PostFundValuation` at the asset's value
(NAV needs units outstanding, so the issuance comes first) → `SetAllocationUnitCap` to
exactly the issued supply (`remaining_capacity == 0`, so no subscription and no further
issuance fits) → open. The investor can still redeem: every gate here keeps the exit open.

**Out of the company's stake (`TransferCompanyStake`).** The 80 % seeded to the company
turned out to belong to a named person. There was no road back out of `CompanyShares`:
the company has no user, so no book order and no escrow, and minting the person a copy of
what the company holds would inflate supply and break the cap table. `TransferCompanyStake`
(`AllocationManage`) is the same `unit_issuances` row with `source = 'company'` (migration
`0038`; a mint is `'mint'`), the same gates (registry in any state, user exists, fresh
NAV, defaulted `units × NAV` basis, the `(service, idempotency_key)` retry contract in the
**same** key space — a mint's key reused for a hand-over is `Conflict`) minus the cap, plus
a Read-First that `CompanyShares(svc).available() ≥ units` (`Validation` otherwise). The
relay posts `Dr UserShares / Cr CompanyShares` under `TransferCode::CompanyStakeTransfer`
(52) with its own transfer id (`tid(issuance, "issue:transfer")`) — a move *between
holders* like a fee clawback in reverse, so `SharesOutstanding` and NAV do not move and
`ListUnitHolders` shows the shift (company −, investors +). `CompanyShares` is debit-normal
with the non-negative flag, so an over-transfer that races the read parks. The recipient's
`fund_positions` projection is the one a mint gets (basis added, high-water mark blended
at the mark); the company has none to reduce. It is a variant of the issuance aggregate,
not a second one, because from the recipient's side it *is* an issuance — units they did
not pay cash for — and the console lists mints and hand-overs as one history
(`UnitIssuance.source` on the wire).

**Retiring units (`RetireUnits`).** The mint's mirror: units burnt out of a holder's
account — a user's or the company's — with no cash leg, for units that should never have
been minted or that stand for an asset the holder no longer owns. Same `unit_issuances`
row with `source = 'retire'` (migration `0039` widens the CHECK), `units` **positive** —
every row carries the magnitude and the source carries the direction, so the 0034 digit
CHECK never learns a sign and the console reads one history of mints, hand-overs and
retirements. Same key contract, same key space (a mint's key reused for a retirement is
`Conflict`). Gates: key first; the allocation registered **and `closed`** — burning a
holder's units out of a live product is a decision that deserves a closed door first,
and `force` is the operator's explicit override (`Precondition` → FAILED_PRECONDITION
without it); a user holder exists; fresh NAV (recorded, default basis `units × NAV` — the
book value written off, not cash that moves); and a Read-First that the holder's
`shares_key(svc).available() ≥ units` — units resting on the book or reserved by a queued
redemption are spoken for and stay (`Validation`). No cap check: supply only shrinks. The
relay posts `Dr SharesOutstanding / Cr <holder shares>` under `TransferCode::UnitRetire`
(53, its own code beside `ShareBurn` for the reason `UnitIssue` sits beside `ShareMint`: a
`ShareBurn` is always paired with a `Redeem` payout, a retirement never is) with its own
transfer id (`tid(issuance, "issue:retire")`); the holder's debit-normal account with the
non-negative flag parks an over-retire that races the read. The `applied` stamp is the
same; for a user holder the projection runs the **seller's** side of a trade — `units −=`,
`cost_basis` shed pro rata and clamped at zero — and the high-water mark is untouched,
because nothing was realised at any price. The company has no projection to reduce.

**Backing — cash vs in_kind.** Every unit a subscription mints has cash behind it: the
investor's claim moved into the fund's, and a redemption pays that cash back out. Units
minted in kind have **none**, so a redemption on such a product asks the fund to pay cash
it does not hold — it queues forever or, once the fund holds cash for some other reason,
pays out money that belongs to someone else. `Allocation.backing` (`domain::allocations::
AllocationBacking`, migration `0039`: `cash` default, `in_kind`) names the distinction,
orthogonal to state and access. The **first in-kind mint flips a `cash` product to
`in_kind`** inside `issue_units`, after every gate and before the row is written
(idempotent; the order is deliberate — a flip with no mint behind it is one operator
command to undo, a mint on a product still `cash` lets the next redemption price units
the fund cannot pay for); `TransferCompanyStake` and `RetireUnits` leave it alone.
Nothing automatic ever flips it back: an operator declares the fund holds cash for the
units with `SetAllocationBacking` (`AllocationManage`, idempotent, either value, audited
as `BackingChanged`). The redeem gate (`allocations_app::require_redeemable`) now runs
both checks off one load — state first, so a draft answers "never open", then
`Allocation::ensure_cash_backed`, which refuses an `in_kind` product as a `Precondition`
(FAILED_PRECONDITION, "units of '<svc>' are not backed by fund cash — sell them on the
book instead of redeeming"). Access is still never consulted on the way out. The
migration backfills by data, not by name: a product with at least one `source = 'mint'`
row is `in_kind` (in production `service_arb` and `test_book`); a pod that predates the
column inserts without it and lands on `cash`.

## The book — holders trading units with each other (`domain::book`, `BookService`)

A subscription is a dealing *with the fund* at NAV; a redemption is the reverse. The
**book** is where holders deal **with each other**: one central limit order book per
allocation, price-time priority, in the shape of a spot exchange (limit and market
orders; `gtc` / `ioc` / `alo` post-only; partial fills; cancel). NAV stays the accounting
price — positions, P&L and the fee high-water mark are still measured at it — and the
book's last trade is a second, market quote shown beside it (`BookSnapshot.nav`). The
book never mints or burns: every unit that changes hands already existed, so supply and
NAV are untouched by a trade. Issue [`#218`](https://github.com/EV-invest/banking/issues/218).

**Who trades.** The registry's *access* axis in full — `require_tradable` refuses a caller
below `invest` (`Precondition`) and answers `NotFound` for a `hidden` product — but **not
its lifecycle**: a `closed` product's holders may still trade among themselves. A book
closes only by its own policy (`book_policies.book_open`, opt-in per product like the fee:
no row is a closed book). Placing is also refused under the read-only kill-switch and for a
frozen owner (the [`OutflowPolicy`] facts, checked in the use case as they are at dispatch);
cancelling never is. The policy — `book_open`, `taker_fee_bps`, `price_tick` (default
`0.01`), `lot_size` (default `0.0001`), `market_slippage_bps` (default 500),
`allow_unbacked_trading` (default `false`) — is set by `AllocationManage`.

**Unbacked trading acknowledgement.** On an `in_kind` product (see
[In-kind issuance](#in-kind-issuance--the-companys-stake-domainissuance-allocationsserviceissueunits))
a redemption is refused and the book is the holders' only exit — which makes the book the
place a buyer pays cash for a claim on an asset the fund holds no cash for, one they cannot
redeem. That is a decision an operator makes knowingly, so the policy carries
`allow_unbacked_trading` and one domain check, `BookPolicy::ensure_tradable_for(service,
backing)`, runs at **two** gates: `set_policy` refuses to open the book on an `in_kind`
product without the flag (`Precondition`, nothing written — the console's early, legible
refusal), and `place_order` refuses every order on an `in_kind` product's open book without
it (the backstop). The second gate is not redundant: the backing flips `cash → in_kind` on
the first in-kind mint, which can land *after* the book opened, and a book that kept
trading would then sell unbacked units nobody acknowledged — so the order is refused with
the reason rather than filled quietly. The flag is harmless on a `cash` product (it
acknowledges in advance) and the terminal shows buyers a notice when it is set. Migration
`0040` backfills it `true` for every book that was open on an `in_kind` product at the
deploy (`service_arb` in production): those were trading unbacked units with the operator's
knowledge, and a deploy must not close a book nobody decided to close. A pre-0040 pod that
UPSERTs a policy mid-rollout resets the flag; the new pod then refuses that product's orders
with the reason and the operator sets it again — short and recoverable.

**An order is an escrow in the ledger.** Two per-user accounts hold what an order has
committed: `BookShares(service, user)` (64, debit-normal, Share ledger,
`book_shares:<svc>:<uuid>`) and `BookCash(user)` (65, credit-normal, USDT ledger,
`book_cash:<uuid>`). A sell locks its size `Dr BookShares / Cr UserShares`
(`TransferCode::BookLock`, the holding's non-negative flag refuses an over-lock); a buy
locks its **worst case** — notional at the limit plus the taker fee on it, `Dr UserClaim
/ Cr BookCash` (the claim's flag does the same). `BookCash` is a claim like any other, so
the global `sum(custody) == sum(claims)` does not move on a lock; the Share-ledger
invariant becomes `SharesOutstanding(svc) == Σ UserShares + Σ BookShares + FeeShares +
CompanyShares`. A holder's position reports the escrowed units as `units_in_orders`
(still theirs, still valued, not free to redeem or sell twice), and the wallet reports the
escrowed cash as `in_orders` — `BookCash` is a second account of the same user, so
without that figure `available` would simply drop by the reserve with nothing on the
wallet explaining where it went. Both escrows stay inside `total` (see
[User wallet](#user-wallet--deposit--withdraw-domainwithdrawals-walletservice)): an order
moves money between two of its terms, never out of it.

**A fill is delivery versus payment or nothing.** Each trade is ONE linked TigerBeetle
chain ([`Ledger::post_linked`], `LedgerAction::PostLinked`): `Dr UserShares(buyer) / Cr
BookShares(seller)` for the units, `Dr BookCash(buyer) / Cr UserClaim(seller)` for the
cash (`BookFill`), and — when the taker owes one — the fee out of the taker's side into
`FeeRevenue` (`BookFee`; a taking seller pays it out of the claim the cash leg just
credited, because linked legs see each other's effect). The chain is idempotent on its
first leg's id: it applies atomically, so the first id existing means every leg did. Fills
land at the **maker's** price; whatever the escrow did not spend when the order reaches a
terminal state — a buy filled below its limit, an IOC remainder, a maker's unused fee
reserve, a cancel — comes back in one `BookRelease` (`Dr BookCash / Cr UserClaim` or `Dr
UserShares / Cr BookShares`). Every id is deterministic: the lock and the release from the
**order** id (`book:lock`, `book:release`), the three legs from the **trade** id
(`book:fill:units|cash|fee`).

**A cancelled order says why.** `cancelled` is reached three ways — the owner's
`CancelOrder`, an IOC limit whose remainder could not fill at once, a market order whose
priced limit ran out of depth — and the last two end with `filled > 0` exactly the way a
user's cancel after a partial fill does. So every cancel carries a
[`CancelReason`](../../domain/src/book.rs) (`user` | `ioc_remainder` | `market_remainder`;
wire vocabulary in `evbanking_contracts::book::cancel_reason`), set exactly when the state
is `cancelled` (`book_orders` checks the pair the way it checks `rejected` ↔
`reject_reason`), and the first reason stands on the idempotent repeat cancel because it is
the one that actually ended the order. The two reason columns are disjoint by construction:
`reject_reason` is the ledger's free text on a `rejected` order, `cancel_reason` a closed
vocabulary on a `cancelled` one; a refused post-only or self-trade order is never recorded
at all, so it has neither.

**Single writer, no in-memory book.** `PgBook::place` is one transaction under
`pg_advisory_xact_lock(hashtext(service))` (a buy also takes the shared per-user claim
lock, a sell the position row): it reads the opposite side best-first (the partial index
`book_orders_resting_idx`, sorted on `price::numeric` — the digit strings do not sort —
then `seq`, the identity column assigned under the lock; `created_at` is the transaction
START and two queued transactions can carry it in the wrong order), runs the pure
[`MatchingEngine`] (`PriceTimeEngine`; the trait is the seam for a different rule), and
records the answer: the taker's row, a `book_trades` row per fill, every maker's new
state, the bumped `book_revisions` counter, and the [`BookEvent`]s drained to the outbox
**in the order the relay must apply them** — `OrderPlaced` (the lock) first, then a
`TradeExecuted` per fill, then an `OrderReleased` for whatever ended. The engine refuses a
whole order rather than filling part of it for a self-trade (the incoming order would hit
the caller's own resting one — skipping it would let them jump their own queue) and for a
post-only order that would take; a market order is priced upstream from the best opposite
quote ± slippage, rounded *outward* to the tick, and then run as an IOC limit (an empty
opposite side is `Precondition`). The depth snapshot, the tape and the candles are
aggregating SQL over the rows; there is nothing to warm up or lose on a restart.

**Read-First, ledger backstop, and the one park that changes state.** The use case checks
the free holding / claim before writing (the same optimistic read as Subscribe, serialized
by the same locks). Two placements can still both pass it before the relay moves the first
— and then the second lock parks on the non-negative flag. Unlike every other park, this
one **must** change control-plane state: an order standing on the book with nothing behind
it would keep matching, and every fill against it would park too. So the relay, after
parking an `OrderPlaced`, marks the order `rejected` (`reject_reason` set) and bumps the
book revision (`infrastructure::book::mark_rejected`) — the order comes off the book and
the caller sees why. Its trades, if any raced in between, park under reconciliation like
any other.

**Cost basis moves on both sides, relay-side.** After the chain posts, `project_trade`
(marker `saga_steps` leg 100, role `book_trade_position`, once per event) adds what the
buyer paid — notional plus the fee when they took — to their `fund_positions` basis and
units, blending the high-water mark at the fund's NAV of the moment (carried on the event:
performance is measured against NAV, so the mark has to be one), and reduces the seller's
basis pro rata as a redemption settle does — **clamped at zero rather than refused**: a
refusal here would be a `Retry` that wedges the single-worker relay, and every unit a
seller can lock came through this same relay in an earlier `seq`. Both writes settle the
accrued management fee first (`fee_accrual::carry_accrual`).

**Idempotent by `client_order_id`**, unique per caller (`1..64`), read before pricing and
again under the lock: a retry lands one order; the same id for a different order is
`Conflict`.

**The live feed** (`WatchBook`) is an in-process `tokio::sync::watch` per book
([`application::book::BookFeed`]), told the revision after every commit: a subscriber
whose wait ends re-reads the book and frames the top-N snapshot, the latest public trades
and `orders_revision` (the revision at which the CALLER's own orders last changed, from
`book_orders.revision`, so a client refetches `ListOpenOrders` only when that moves).
`watch` rather than `broadcast` so a slow socket coalesces to the latest state instead
of building a backlog; no `LISTEN`/`NOTIFY`, because the hub is one instance. Nobody
else's orders and no balances ride on a frame.

## Fees — "2 and 20" (`domain::fees`, `FeesService`, `FeeSweeper`)

A fund charges two things, and they answer different questions. **Management** (2% p.a.)
is rent on parked capital: it accrues continuously with elapsed time and is charged
whether the fund made money or not. **Performance** (20%) is a share of profit above a
**high-water mark**, crystallized at the end of a period. A `ServiceId` with no
`fee_policies` row charges **nothing** — the fee is opt-in per product, so it can never
appear on a fund whose prospectus did not promise it. `FeePolicy::HOUSE` is the default
shape (200 bps / 2000 bps / no hurdle / invested capital / annual).

**The management base is the investor's *invested capital*, not the market value** —
`ManagementBasis::InvestedCapital`, the position's `cost_basis`. Market value is
available (`MarketValue`, the hedge-fund convention) but is not the default, and the
reason is a trust seam: the AUM the operator posts is what sets NAV, so a
market-value base would let an operator raise their own fee with the same input the
NAV-move guard already treats as "the most dangerous seam in the system". Invested
capital is the number the investor agreed to, and no operator input moves it.

**The mark is per investor, on `fund_positions.high_water_mark`** — reserved by `0005`
"so adding a performance fee later isn't a painful backfill", and this is that
follow-up. A *fund-level* mark (what a tokenized vault is forced into, because a share
token cannot remember who bought when) mutualizes the fee: someone who subscribes after
a drawdown rides the recovery fee-free while someone who subscribed at the top pays on
gains that only restored their own loss. The industry's fixes are *series accounting* (a
share class per dealing day) and *equalization* (per-investor credits against one
class); the position projection already exists per `(user, service)`, so the third road
is simply to put the mark on it. The mark is maintained by the subscribe projection as
`GREATEST(existing, subscription NAV)` — deliberately the **investor-favourable** blend
on both sides (a top-up above the mark raises it for the whole position; one below
leaves it), where exact per-lot fairness would need series accounting. `0023` backfills
pre-existing positions to `cost_basis / units`, their own average entry price: a mark
left at the reserved `'0'` would treat an investor's whole NAV as profit and charge 20%
of their **capital** on the first crystallization.

### The charge moves units, never cash — and that is the whole design

A charge is one posted transfer on the **Share** ledger: `Dr FeeShares(svc) / Cr
UserShares(svc, user)` (`TransferCode::FeeClawback`). `FeeShares` (code 62,
debit-normal, `shares_fee:<svc>`) is a holder of units exactly like a user. Three
properties follow, and each is pinned by a test in
[`tests/fee_policy.rs`](tests/fee_policy.rs):

- **No chain fee, ever.** Nothing leaves custody when a fee is charged, so the fund pays
  no gas per investor per period. The manager converts an accumulated unit balance to
  cash *once*, in bulk (below), instead of N times.
- **An investor cannot be pushed negative.** The cash claim is not touched *at all*, and
  the clawback is capped by the units actually held — with TigerBeetle's
  `credits_must_not_exceed_debits` flag on `UserShares` as the ledger backstop under the
  application's own cap.
- **Nobody else pays.** `SharesOutstanding` does not move (a transfer *between holders*,
  not a mint), so NAV per unit is unchanged. This is what makes the per-investor mark
  honest rather than a dilution everyone shares. The Share-ledger invariant becomes
  `SharesOutstanding(svc) == Σ_user UserShares(svc, user) + FeeShares(svc)` (plus
  `CompanyShares(svc)` once the company holds an in-kind stake — see above).

What cannot be collected — the holder's units are locked by a queued redemption or
escrowed by a resting sell order on the book, or the charge floors below one base unit of
share — is carried as `fund_positions.fee_debt` and taken on the next assessment. It is
never written off and never becomes a negative balance. Only a charge that is not **owed**
at all (a zero policy, a clock that did not run, a residue flooring to nothing) persists
nothing and leaves both clocks where they are, so the accrual simply continues into the
next sweep (`FeeCharge::is_empty` is `due.is_zero()`, not "nothing collected").

**The fee is owed on the whole position and collected from the free holding** (#255).
`PositionSnapshot` carries two unit figures: `units`, the position the fee is measured on
— `UserShares.posted + BookShares(svc, user).posted`, so a resting sell's escrow and a
queued redemption's reserve both count, because an order or a pending exit changes where
the units sit and not who owns them (and `MarketValue` management is priced on all of
them) — and `collectable`, the holding's **available** balance, which caps the clawback.
Both escrows fall outside the cap: a queued redemption is a pending debit (`locked`), a
resting sell has moved its units into `BookShares` outright. So an order changes *which*
road the fee takes but not the amount — with some units free, the charge takes those and
carries the rest as debt; with every unit in an order, the charge is still recorded, with
`charged_units = 0` (`0041` widened the audit row's CHECK for exactly this), the whole
amount lands in `fee_debt`, the clocks move, and the next assessment after the order ends
collects the debt plus only the seconds since. `FeeAssessment::record` raises `Charged`
only when `charged_units > 0`, so the relay never sees a zero transfer; the audit row, the
debt and the clocks commit either way. The escrow itself is never drawn on — it belongs to
the order until the book releases it — and the holding cannot go negative, because the cap
floors at what it has. Pinned by the three escrow tests in
[`tests/fee_policy.rs`](tests/fee_policy.rs).

The alternative — pausing the clock while the units are escrowed, and telling the holder
— was rejected because it *is* the hole this closes: a holder who keeps an ask resting
defers their fee for as long as they like and is billed the whole stretch in one blow the
day it comes off, while the fund carried their capital the entire time. The debt road
already existed (`0023`, [`fee_accrual`](src/infrastructure/fee_accrual.rs)) and adds no
new state.

The residual window is the few milliseconds between a placement being recorded and the
relay applying its lock, during which an assessment still sees the units as free: a
clawback landing there makes the lock park on the non-negative flag and the order is
marked `rejected` — the designed backstop, not a negative balance.

### Ordering, clocks, and the atomic write

`assess` charges **management first, performance on what is left**: charging both
against the same pre-fee unit count would take a performance cut of capital the
management fee has already claimed. Two clocks are tracked separately because they mean
different things — `fees_accrued_at` moves on **every** charge (management is
continuous), `crystallized_at` only when the performance fee actually crystallized, so a
mid-period charge cannot silently restart the period.

`PgFeeAssessments::charge` commits four things in one transaction under the
`fund_positions` row lock: the `fee_assessments` audit row, the new debt/mark/clocks,
the projection's `units` **decremented by the clawback**, and the `Charged` event into
`event_log` + `outbox` (absent when nothing was collected — the first three still
commit). The `units` decrement is load-bearing and easy to miss: the
redemption settle reduces cost basis by `(units − redeemed) / units` against the
*projection's* count (`0010`), so a clawback that took units without telling the
projection would leave that denominator permanently too large and every later settle
would under-reduce the basis. The row lock is the same target the settle takes, which is
what keeps the two from interleaving.

**A fee is priced at the dealing NAV, staleness guard included.** That is a safety
property: if an operator stops posting marks, fees stop accruing rather than accruing
against a price nobody has confirmed. The fee is the one charge the operator sets,
prices and collects, so it gets the same guard as an investor's own dealing.

### Settlement — the one moment a fee becomes cash

`SettleFeeShares` (operator, `AllocationManage`) converts a fund's accumulated fee units
in one bulk operation. Two posted legs, **burn-first** like a redemption settle: post
`Dr SharesOutstanding / Cr FeeShares` (`ShareBurn`), then `Dr ServiceClaim / Cr
FeeRevenue` (`FeeSettle`). The payout leg joins `redeem_payout` in the relay's
settle-time liquidity pre-check, so a fund short of cash parks the whole event with
nothing applied rather than burning units it cannot pay for. The application layer
**refuses** rather than queueing when the claim is short: unlike an investor's
redemption nobody is waiting on this, and unconverted fee units keep accumulating at no
cost.

### The sweeper

[`fee_sweeper`](src/infrastructure/fee_sweeper.rs) is a `join!` branch beside the
recovery jobs — the only one that *charges* rather than repairs. Hourly, it takes up to
500 unit-holding positions whose last accrual is over 24h old, oldest first, and
assesses each. The cadence is an efficiency knob only: the fee owed is a function of
elapsed seconds, so missing a week bills that week on the next pass with no drift and no
double-charge. Per-position failures warn and continue — one investor's stale NAV must
not stop everyone else's fee from being collected.

**Crystallization frequency is a price, not a detail.** Moving from annual to quarterly
measurably raises what an investor pays over a fund's life, because each reset locks in
gains a later loss can no longer claw back. Annual is the default for exactly that
reason.

### Known gap — exit crystallization

`Trigger::Redemption` (crystallize only the units leaving, at the redemption-day NAV,
leaving the mark for the units that stay) is modelled and unit-tested but **not wired**
in v1: only `Trigger::Period` runs. Wiring it at *request* time would double-charge the
same gain if the redemption were cancelled and re-requested, and over-charging an
investor is the worst failure this feature has. The correct hook is the redemption
**settle**, netting the fee from the payout (`Dr service / Cr fee` beside the existing
`Dr service / Cr user`, mirroring the withdrawal settle's fee leg) — where it is
terminal, idempotent, and cannot be replayed. Until then an investor who redeems shortly
before a period end escapes the performance fee on that period's gain.

## User wallet — deposit & withdraw (`domain::withdrawals`, `WalletService`)

A user's money is **one** network-agnostic claim (`user:<uuid>`). `GetWallet` presents it
segmented by lifecycle — `available` (`posted − locked`), `in_orders` (the cash the
book holds for the user's resting buy orders, `BookCash` posted — still theirs, handed
back as the orders fill or are cancelled), `invested` (the value of the user's fund
positions, `Σ (units + units_in_orders) × current NAV` — the units a resting sell escrowed
are still the holder's and still valued, exactly as the position reports them),
`pending_withdrawal` (the claim's `locked`: queued/in-flight withdrawals), and `total =
available + in_orders + invested + pending_withdrawal`. The cash terms come off two
posted balances read in one pass (the claim's `posted` is `available + pending_withdrawal`
by construction, `BookCash` is the third term), so placing or cancelling an order moves
money between terms without moving the sum — plus a
per-rail deposit address and a per-rail **withdrawable** view (`instant = min(available,
rail liquidity)`, the accept-and-queue hint — it discloses a rail's liquidity only up to
the user's own balance; bucket/round it if that must stay private).

**Deposit (chain → claim).** `GetDepositAddress` hands a **verified** user (`kyc_level ≥ 1`
— see [Authorization](#authorization-defense-in-depth)) a stable per-(user, network)
address from the [`DepositAddresses`] port — a stub HD-derivation cached into
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
when the rail treasury provably lacks the net on-chain, or when the **outflow policy** no
longer permits the payout. That policy — the read-only kill-switch, the owner's
cross-plane freeze, and the tier-1 floor for a `WithdrawalSource::User` — lives inside
`dispatch_withdrawal` itself, not in its callers: admission is not the last word, because
a withdrawal can sit `Queued` for hours and a pause, an AML hold or a revoked
verification landing in that window must still stop the money. Every arm fails **closed**
(a missing owner row, an unreadable flag and a corrupt tier all refuse), and both the
sweep and the operator RPC inherit it from the one place rather than each carrying a copy.
The operator handle has **no `force` override**: the override for a pause is
`SetOperationsMode`, which is itself permissioned and leaves one auditable record of who
reopened outflows, instead of a per-payout flag indistinguishable from a routine
dispatch. `SettleWithdrawal` and
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

### Revenue payout — the same saga, sourced from the fund

The fund earns money in two places: the **retained fee** on every user withdrawal, and the
settled 2-and-20 (§Fees → *Settlement*). Both land in one claim, `fee`. Paying that money
out is the same claim → chain direction a user withdrawal is, so it is the **same saga**,
not a parallel one: `Withdrawal` names its origin with [`WithdrawalSource`] instead of
assuming a user, and the queue, the chain watchers, the dispatcher, the reaper and
reconciliation cover payouts with no new machinery.

`RequestRevenuePayout` (Admin/Owner, `authz::RevenuePayout`) Read-First checks the **`fee`
claim's** available balance and reserves the gross as `Dr fee / Cr clearing`. Two
consequences worth stating plainly, because both are load-bearing:

* **Client money and seed capital are unreachable from here.** They are different ledger
  accounts (`user:<uuid>`, `fund:<net>`), so the cap is not a filter someone could forget
  to apply — it is the `fee` claim's own balance, with TigerBeetle's non-negative flag as
  the backstop underneath.
* **A payout charges no fee.** The fee claim is *where* fees are retained; charging one
  would credit the money straight back to the account it was just debited from.

A payout has no owner in the identity plane — the fund is not a user — so the gates that
read a user's control-plane flags simply do not apply: the dispatch policy's per-owner
arms are behind `if let Some(owner)`, so there is no row to read and nothing to fail
closed on. The kill-switch still applies, because it pauses outflows, not users. Everything else is
identical, **including the cardinal rule**: once a payout's broadcast may have reached the
chain, failing it double-pays.

Old outbox payloads spell the field `user` and hold a bare UUID. `WithdrawalSource`'s
string form reads those unchanged, so a row parked before this landed still drains after it.

## Operations — the activity timeline (`OperationsService`)

The first surface here that is **only** a read model. The four things a user does —
deposit, withdraw, subscribe, redeem — are separate aggregates, each already listing its
own history. What none of them can answer is "what happened, in order": interleaving four
client-side lists needs a timestamp they all agree on, and a per-source `LIMIT` cannot
produce the newest N *overall* (a long deposit history would push out a recent
redemption). `ListOperations` is that join, done once in the hub as a single `UNION ALL`
over the projections the write side already maintains — so it introduces no new table, no
new event, and no new write path.

It owns nothing, so it gets a **plain driven port** ([`ports::operations`]) rather than a
`Repository`: there is no aggregate to hang the marker on, the same shape [`Deposits`]
takes. In Rust the row is a **sum type** (`Operation::{Deposit, Withdrawal, Subscription,
Redemption}`) so a deposit cannot carry a NAV; on the wire it flattens to one message with
a `kind` discriminator, because the whole surface is projected to TypeScript through
OpenAPI and a flat shape survives that pipeline unambiguously. The service layer is where
that trade is paid, once.

Paging is a `limit` (default 100, capped at 200) with no cursor — the timeline is a
recent-activity surface, not an export. The adapter over-fetches one row to answer
`truncated` honestly rather than running a second `COUNT` over the same union. Ordering is
`created_at DESC, id`: on the `timestamptz`, not on truncated epoch seconds, so a
subscribe/redeem pair made in the same second keeps the order the user made it in.

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
  **aggregate-derived** id (`tid(aggregate_id, <salt>)` — `BURN_RESERVE` for a redemption,
  `CLEARING_RESERVE` for a withdrawal) so its settle/cancel can recompute the same
  `pending_id`.
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
  to `clearing`, not the rail.) The guard is **idempotent under redelivery**: legs apply as
  separate TB calls, so a transient failure after the disburse (the fee leg, or its own
  saga-step insert) redelivers an event whose re-read rail balance already reflects the
  disburse's own outflow — re-checking it would double-count and spuriously park a
  half-applied settle. The pre-check therefore asks TB (`Ledger::transfer_exists`, the
  authority — the saga step may have failed to record) whether the guarded leg's
  deterministic id already applied, and skips its guard if so; the re-apply is `Exists`.
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
  first: `pg_advisory_xact_lock` keyed on the claim ([`outbox::lock_claim`], of which
  `lock_user` and `lock_revenue_claim` are the two named cases), held to commit. The key for
  those two is **frozen** at what every released binary computes — the raw user id, and a
  fixed v5 name for `fee` — so a rolling deploy cannot leave old and new writers serializing
  on different targets; `the_generalized_claim_lock_keeps_the_keys_the_old_helpers_computed`
  pins it.
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
| `GetTreasury` / `SeedCapital` / `RecordDeposit` | operator | `require_permission` (RBAC matrix) | chain-proven arrival (amount + party read off the chain) ∧ `tx_ref` gate |
| `Subscribe` | the user | `sub == user`, `is_access`, **not revoked, not paused, not frozen** | available claim ≥ cash ∧ fresh NAV (TB flag backstop) |
| `Redeem` | the user | `sub == user`, `is_access`, **not revoked, not paused, not frozen** | available units ≥ amount ∧ fresh NAV (TB flag backstop) |
| `CancelRedemption` | the user | `sub == user`, `is_access` | owns it ∧ state is `queued` (idempotent) |
| `GetPosition` / `ListPositions` / `ListRedemptions` / `GetFundNav` | the user | `sub == user` | — |
| `GetWallet` / `ListWithdrawals` | the user | `sub == user` | — (`GetWallet` serves an address only at `kyc_level ≥ 1`) |
| `GetDepositAddress` | the user | `sub == user` | `kyc_level ≥ 1` (else `permission_denied`) |
| `RequestWithdrawal` | the user | `sub == user`, `is_access`, **not revoked, not paused, not frozen** | owner not frozen (`frozen ∨ disabled`, via `OutflowPolicy::standing`) ∧ `kyc_level ≥ 1` ∧ available claim ≥ gross (TB flag backstop) |
| `CancelWithdrawal` | the user | `sub == user`, `is_access` | owns it ∧ state is `queued` (idempotent) |
| `DispatchWithdrawal` | operator (treasury) | `require_permission` (RBAC matrix) | state is `queued` (idempotent) ∧ **not read-only** ∧ (user source) owner not frozen ∧ `kyc_level ≥ 1` — fail-closed, no `force` |
| `SettleWithdrawal` / `FailWithdrawal` | operator | `require_permission` (RBAC matrix) | state is `processing` (idempotent) |
| `PostFundValuation` | operator | `require_permission` (RBAC matrix) | allocation registered ∧ units outstanding > 0 ∧ NAV move ≤ threshold vs the previous mark ∧ vs the rolling-window anchor — **no override**; beyond it: `ConsiliumService.OpenValuationOverride` (same `ValuationPost` permission, initiator must hold an owner seat, owners' quorum executes the mark) |
| `Redeem` / `SettleRedemption` (cooldown) | the user / operator | as above | the redeeming user posted **no** mark for this fund within `VALUATION_REDEEM_COOLDOWN_SECS` (`failed_precondition` otherwise; checked at request and again at settle) |
| `IssueUnits` / `ListUnitHolders` | admin (`AllocationManage`) | `require_permission` (RBAC matrix) | (issue) allocation registered ∧ user holder exists ∧ fresh NAV ∧ issued + units ≤ cap; idempotent by `(service, idempotency_key)` |
| `TransferCompanyStake` | admin (`AllocationManage`) | `require_permission` (RBAC matrix) | allocation registered (any state) ∧ user exists ∧ fresh NAV ∧ `CompanyShares.available ≥ units` (TB flag backstop); no cap (supply unchanged); idempotent by `(service, idempotency_key)`, shared with `IssueUnits` |
| `PlaceOrder` | the user | `sub == user`, `is_access`, **not frozen**, **not read-only** | allocation visible ∧ `invest` (state ignored) ∧ `book_open` ∧ on tick/lot ∧ free units / claim ≥ escrow (TB flag backstop → `rejected`); idempotent by `client_order_id` |
| `CancelOrder` | the user | `sub == user`, `is_access` | owns it ∧ state is resting (idempotent on cancelled) |
| `ListOpenOrders` / `ListOrderHistory` / `ListUserTrades` | the user | `sub == user` | — |
| `GetBook` / `ListTrades` / `ListCandles` / `WatchBook` / `GetBookPolicy` | the user | `sub == user`; allocation visible to the caller (`AllocationManage` sees all) | — |
| `SetBookPolicy` | admin (`AllocationManage`) | `require_permission` (RBAC matrix) | allocation registered; bps ≤ 10000, tick and lot > 0 |
| `SettleRedemption` / `FailRedemption` | operator (treasury) | `require_permission` (RBAC matrix) | state is `queued` (idempotent) ∧ (settle) position projection tracks ≥ the redeemed units |
| `GetUserBalance` | operator | `require_permission` (RBAC matrix); resolves the CONCIERGE id first via the bridge mirror (`users.concierge_user_id`), then the banking id; unknown ⇒ `NOT_FOUND` | — |
| `ListParkedEvents` | operator | `require_permission` (RBAC matrix) | — |
| `UnparkEvent` | admin (`OutboxManage`) | `require_permission` (RBAC matrix) | parked ∧ not dispatched ∧ **not compensated** (the double-apply guard) |

The `kyc_level ≥ 1` arms above are the **enforced** position of the deployment switch
(`KYC_GATE_ENABLED`, enforced unless explicitly lifted — see **The switch** under the
verification gate below); every other arm in this matrix is unconditional.

`require_permission` (`services::support`) is `is_access` + the pure RBAC matrix
(`domain::authz::grants` — the single place the matrix is defined) over the caller's
bridge-mirrored role, **after** the account gates, in the money-path gate's order: a stale
`token_version` is `unauthenticated` first, then a `disabled` (or frozen) operator is
`permission_denied`. The mirrored `users.role` column is the
**only** source of the role — there is no environment-driven override, so a caller with
no local row holds nothing and a non-UUID subject is refused outright.

**Money-path gate** (`services::support::unfrozen_caller`): the value-leaving RPCs above
(`Subscribe`/`Redeem`/`RequestWithdrawal`) run one `IssuanceTarget` resolve and one
kill-switch read per call and refuse, in this order: `unauthenticated` when the token's
`token_version` is below the folded revoke floor (the same floor `require_permission` holds
operators to — answered first, so a revoked caller learns nothing about the platform's
state); `failed_precondition` when outflows are paused; `failed_precondition` when the
caller's banking row is `frozen` or `disabled`, or has no row at all (fail-closed). The
decision is the pure `money_caller_gate`, unit-tested for the ordering. `frozen` is set
by the one-way concierge→banking lifecycle bridge consumer (`infrastructure::bridge`),
which PULLS `UserLifecycleEvent`s from the
concierge plane (`UserEvents.PullUserLifecycle`, `BRIDGE_SERVICE_TOKEN`) and mirrors
SUSPENDED→frozen / REINSTATED→unfrozen, KYC, and the revoke floor onto `users`, keyed by
`auth_subject` and dedup/ordered by per-user `sequence`. Identity stays owned by concierge;
banking only mirrors the gating slice. The gate fails CLOSED (UNAVAILABLE) if the flag can't
be read. Cancel/read RPCs are intentionally NOT gated, so a frozen user can still unwind
queued positions.

**Nothing is consumed without being applied.** `bridge_cursor` is a single global position
and the concierge only re-delivers *ahead* of it (`WHERE position > after_position`), so an
event the consumer walks past is gone for good — there is no dead-letter to recover it from.
Two things can stop an event from applying, and they get opposite treatments because they
are unblocked by opposite things. **A subject with no local row** (never signed in here, or
a CREATED that aged out of the outbox before banking was deployed) is parked in
`bridge_deferred_event` and replayed by the consumer's own sweep the moment the row appears,
from either direction — a later CREATED or a first sign-in. The cursor moves on, so one
orphan subject cannot wedge the mirror for every other user; the price is a table whose
columns must carry every field `apply` reads. **A `kind` this build cannot name** is
different: the concierge ships ahead of banking, so an unknown kind is the ordinary shape of
a mid-rollout event, and *nothing local can ever interpret it* — only a newer binary can.
The cursor stops on it (head-of-line, and the rest of the batch is left unapplied so a later
event can't advance that subject's guard past it), leaving the event in the concierge outbox
at full fidelity until the upgrade lands. Marking it applied — which is what bumping
`last_lifecycle_sequence` "so it isn't re-fetched forever" did — is how a freeze or a tier
revocation gets swallowed while the money plane keeps trading under withdrawn rules.

**Verification gate (`kyc_level`).** The mirrored tier is not just stored, it *gates*:
`domain::users::KYC_LEVEL_VERIFIED` (= 1) is the floor for money crossing the platform
boundary in either direction — `GetDepositAddress` and `RequestWithdrawal`, and again at
**dispatch** (`dispatch_withdrawal`), since acceptance and payout can be hours apart and a
`KYC_CHANGED{kyc_level: 0}` sets no freeze for the freeze gate to catch. The ladder is
written down on `banking.v1.UserProfile.kyc_level`: **0** registered (confirmed email,
nothing verified), **1** verified (document + liveness + face match + a passed
sanctions/PEP screen), **2** enhanced (proof of address + source of funds, raised limits),
**3** elevated (EDD, human-set only). Concierge owns the value and an admin sets it there;
banking has no transition that writes it, and the `users` UPDATE deliberately omits the
column so a profile save can never race a `KYC_CHANGED` back to an older tier.

Both gates sit **above** the [`DepositAddresses`] port for the same reason the rail gate
does: the first `address` call provisions a signer keypair, and a key minted for an
unverified user is an address the fund must watch, sweep and account for forever. The two
refusals stay distinguishable at the wire — an unconfigured rail is `Ok(None)` ("this rail
cannot fund you"), an unverified caller is `Forbidden`/`permission_denied` ("finish
verification") — because the cabinet has to pick a different screen for each. `GetWallet`
stays fully readable at tier 0 (a user's own balance is never hidden from them) but serves
no address on any rail. A **revenue payout is not gated**: it pays the fund's own earned
revenue out of the `fee` claim and has no user behind it to verify.

**The switch (`KYC_GATE_ENABLED`).** The verification floor is the one of the two deposit
gates that can be turned off — it guards a rule the platform chose, where the rail gate
guards a fact about the chain — so it is a deployment switch, `config::KycGate`, read from
the environment **once at boot** and carried on `AppState` rather than re-read per call.

| `KYC_GATE_ENABLED` | gate | effect |
| --- | --- | --- |
| unset / empty / `true` / `1` | **enforced** (the default, and what production runs) | tier 0 gets no deposit address and cannot withdraw |
| `false` / `0` | lifted | every tier is issued an address, admitted and paid out — the pre-switch behaviour |
| anything else | **enforced**, with a WARN naming the value | a typo never opens the gate |

Every uncertain reading resolves to *enforced*: this decides whether unverified money
crosses the platform boundary, so it opens only on an explicit, unambiguous word. That is
also why it is not `bool_env`, which reads anything it does not recognise as `false` — the
right default for an opt-in sweep, the wrong one here. An unparseable value warns rather
than refusing the boot, because the reading that cannot lose money is already available
without stopping the hub. Boot logs the position either way (`warn!` when lifted), so a gate
that is off is visible in a pod's first lines and not only in someone's memory.

The same value **must** reach all three gates — address, admission, dispatch. Lifting it at
admission only would accept an unverified user's withdrawal and then leave it queued
forever, since `dispatch_withdrawal` re-checks the tier when the money actually leaves;
that is worse than having no switch at all, and
`tests/kyc_gating.rs::a_lifted_gate_both_admits_and_dispatches_an_unverified_withdrawal`
exercises one withdrawal across both points so the two cannot be wired apart again.

## Reconciliation + reaper + dispatcher (recovery jobs)

TB always wins; the jobs run as `join!` branches of the composition root next to the
relay, on the relay's dedicated pool.

[`reconciliation`](src/infrastructure/reconciliation.rs) (`Reconciliation::scan`) asserts
and **alerts** (Sentry-shipped `error!`, no auto-write) on: the **global** posted
`sum(custody) == sum(claims)` on the USDT ledger (read straight from TB via
`Ledger::cash_invariant`); `clearing`'s reserved (pending + posted) balance vs the gross of
every `queued`/`processing` withdrawal in Postgres; and a scan of every `outbox.parked_at`
row (with its `last_error` and `compensated_at`). The `last_error` column on a
parked row is the first place to look when money didn't move.

[`treasury_drift`](src/infrastructure/treasury_drift.rs) (`TreasuryDrift::scan_once`) is the
**per-rail** counterpart, hourly and alert-only. The invariant above relates two TigerBeetle
accounts, so it stays green when the ledger and the CHAIN disagree; this compares
`wallet:<net>` against the wallets it claims to describe — `treasury_liquidity` **plus**
`deposit_address_liquidity`, because the ledger counts un-swept deposit-address funds too. A
surplus means USDT arrived without a ledger fact (it is also unspendable, since the dispatch
gate mins the two); a shortfall means claims aren't backed on-chain. A divergence must survive
two consecutive scans to be reported, so a transfer landing between the two reads is not an
alert. Rails with no chain view are skipped.

The same job watches **native-coin gas**, a separate failure — a rail can hold exactly the
USDT it should and still be unable to move any of it. `Custody::treasury_gas_runway` reports
how many withdrawals the treasury can still pay for (an EVM rail prices `gas_limit × gas_price`
live; TON divides by its fixed `msg_value`), so one threshold means the same thing on every
rail and `0` is precisely where `ensure_treasury_funded` starts parking. Exhausted is an
`error!`, thin-but-working a `warn!`. Nothing else looked at the native balance before this:
the first symptom of an empty treasury was a user's withdrawal parking.

Out-of-band arrivals are **credited automatically**: each deposit watcher also watches its
rail's treasury and records an arrival as `Party::Piggybank` (`Dr wallet:<net> / Cr fund`),
idempotent by the same `tx_ref` machinery as a user deposit. The sweep moves USDT from a
derived address INTO the treasury and that dollar is already in `wallet:<net>`, so a credit
only fires when the sender is outside every wallet we control — see `is_external_source` in
both watchers. `RecordDeposit` (admin, `CapitalManage`) is the manual path for anything the
watchers could not see, and `SeedCapital` is the same verification with one extra assertion
— "this is the fund's own money": it refuses a transfer that landed on a user's deposit
address instead of crediting that user, so an operator who meant capital and got a deposit
finds out. Both are chain-proven and idempotent by the chain `tx_ref`; neither accepts an
amount from the caller (the free-amount, no-dedup `SeedCapital` was removed in #234).

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
(idempotently, via the same row-locked command as the admin RPC, which is also where the
outflow policy is enforced), so a rail top-up self-heals the queue within one interval.
The sweep keeps exactly one policy check of its own — a whole-cycle short-circuit on the
kill-switch, through that same gate function — so a paused platform costs one read per
interval rather than one refusal per queued row. A treasury read `Err` skips that cycle (the
automatic path stays conservative; the operator RPC may still exercise judgment). Together
with the reaper this brackets accept-and-queue: dispatched within ~30s of a top-up, or
auto-cancelled (refunded) at 24h — the de-facto rail top-up SLA.

## Tests

### Bring-up

Every suite under `piggybank/core/tests` runs against a **real** Postgres and TigerBeetle
through `tests/common/mod.rs` — no suite opens its own connection. `pool()` connects to
`DATABASE_URL` and applies the migrations; `seeded_ledger()` connects to
`TIGERBEETLE_ADDRESS` / `TIGERBEETLE_CLUSTER_ID` (default `127.0.0.1:3033`, cluster `0`)
and seeds the singleton accounts; `database_url()` is for the two suites that size their
own pool. Locally:

```bash
nix run .#db          # the shared postgres cluster; ensures the `banking` database
nix run .#tb          # a single-replica ledger on :3033
DATABASE_URL=postgres://postgres@localhost:5432/banking cargo test -p piggybank-core --tests
```

A missing service is a **skip locally and a failure under CI**. Without `DATABASE_URL`
(or with a replica that does not answer) a suite prints a notice and returns early — the
counters still read `N passed`, so the signature of a run that executed nothing is
`finished in 0.00s`, not a zero. Under `CI=true` — GitHub Actions exports it, and
`nix run .#check-rust` exports it too — the same condition panics with
`integration services required in CI`, so a green tick means the money plane was actually
exercised (#259). `check-rust` brings up its own throwaway Postgres (`:54329`) and
TigerBeetle (`:3039`) from the flake apps, runs the workspace against them and tears them
down on exit; move the ports with `CHECK_POSTGRES_PORT` / `CHECK_TIGERBEETLE_PORT`.

Your own database, when the shared one is busy or on another branch's schema:
`PGDATABASES="banking_<name>" nix run .#db` creates it on the shared cluster; point
`DATABASE_URL` at it. Never downgrade a database across branches — a database that ran
another numbering of a migration fails every suite with `migration N was previously
applied but has been modified`; `dropdb` and `createdb` it instead. A second ledger next
to the default one: `TIGERBEETLE_PORT=3034 TBCLUSTER=1 nix run .#tb`, then
`TIGERBEETLE_ADDRESS=127.0.0.1:3034 TIGERBEETLE_CLUSTER_ID=1`.

`domain` unit tests cover the money + NAV math (incl. the `mul_div` overflow bound and the
share-key ledger sides), the subscription/redemption aggregates, the withdrawal
transitions, the allocation registry's state machine, and the fee arithmetic (the flat-year
2%, the 20% taken net of management, the mark refusing to charge a mere recovery, the
hurdle, the deferral of an uncollectable charge into debt — recorded, not skipped, and
raising no clawback event — and a backwards clock charging nothing);
`piggybank/core/tests/allocation_registry.rs` covers the gate against real Postgres +
TigerBeetle (an unregistered service refused *before* any money moves, a draft taking
nothing, a closed allocation still redeeming, double registration as a conflict, the
catalog's listed/unlisted split, and allocation events staying out of the outbox), and the
in-kind issuance (units landing on a user and on the company with no cash leg and
`ListUnitHolders` / `FundNav.company_units` reporting the split, the idempotency key
returning the same row and minting once while refusing a different request, the
defaulted `units × NAV` basis, the registry/holder/cap gates, the 20/80 recipe ending at
`remaining_capacity == 0` with the investor still able to redeem, and the event reaching
the relay as its own kind) and the hand-over out of the company's stake (the 13 000
moving company → user with `SharesOutstanding` unchanged and `ListUnitHolders` showing
the shift, the recipient's basis and mark, the shared key space refusing a mint's key
and returning a repeat, more than the company holds refused before anything is written,
and an unregistered service refused), the retirement (units burnt out of an investor and
the company on a closed product with the supply, `ListUnitHolders` and the investor's
`fund_positions` units and basis shrinking and the mark untouched, a live product refusing
without `force` and burning with it, the shared key space returning a repeat and refusing
a mint's key, more than the holder has **available** refused before anything is written —
including units a queued redemption has reserved — and the widened `source` CHECK), and
the backing (a registration landing on `cash`, the first mint flipping it to `in_kind`
with one `BackingChanged` fact and a second mint or a hand-over leaving none, the
operator's `set_backing` idempotent and the next mint flipping again, `Redeem` refused as
a precondition on an `in_kind` product before any redemption is recorded and passing
once the operator declares cash — on a closed product too — and a row written by the
pre-0039 INSERT reading as `cash` with the column CHECK refusing anything else);
`piggybank/core/tests/balance_allocations.rs` and
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
`Custody` adapter with a configurable treasury view; plus the dispatch-time outflow
policy — a tier revoked after acceptance stops the sweep, the admin dispatch is refused
under read-only, under a freeze and at tier 0, and a withdrawal whose owner row is gone
fails closed). `piggybank/core/tests/book.rs` hits real Postgres + TigerBeetle for the book: a crossing
limit buy settling delivery-versus-payment with the taker's fee on `FeeRevenue`, both cost
bases and `units_in_orders` moving, supply untouched; a buy below its limit getting the
price improvement back; a partial fill leaving the maker resting; an IOC releasing its
remainder; post-only and self-trade refused with nothing written; a market order priced off
the best quote and refused on an empty side; cancel returning the escrow, idempotently; the
gates (a closed book, `view`, `hidden`, a closed allocation still trading, read-only, a
frozen owner); tick/lot/balance refusals before any write; `client_order_id` retry vs
reuse; a raced over-lock parked by the ledger and the order marked `rejected`; candles and
the 24h change; the feed framing per change with the caller's `orders_revision`; the
policy's defaults and bounds; the wallet showing a resting buy's reserve as `in_orders`
(available down by exactly that, `total` unmoved, a resting sell still inside `invested`)
and every figure back after the cancel; and the three `cancel_reason`s landing on the
row — `ioc_remainder`, `market_remainder`, `user`.
`piggybank/core/tests/fee_policy.rs` hits real Postgres + TigerBeetle for the fee plane's
three load-bearing properties — a charge moves **units** and leaves every cash account
untouched, `SharesOutstanding` is unchanged so no other holder pays, and two investors at
the same NAV owe different fees when they entered at different prices — plus the bulk
settlement (the only moment a fee becomes cash), its refusal when the fund's claim is
short, the sweeper end to end, a fund with no policy never being charged, and the book's
escrow against the clawback — every unit in a resting sell is charged as debt with nothing
collected, the audit row written with `charged_units = 0`, the clocks moved and the ledger
untouched, the debt (and only the seconds since, never the year again) being collected
once the order is cancelled; most units in one caps the charge at the free holding,
carries the rest as debt, and collects it on the next assessment after the cancel, with
the holding exactly empty and the escrow untouched either way. Note that
the accrual clocks are DB-stamped while `now` is caller-supplied, so those tests overshoot
a period boundary by an hour and compare amounts with a tolerance rather than for equality;
the sub-second jitter is 3e-8 of a year's fee and never accumulates. `piggybank/core/tests/kyc_gating.rs` covers the verification floor against real Postgres +
TigerBeetle: tier 0 is refused a deposit address **without the address gateway being reached
at all** (a call counter, since a gate placed after the port would look identical from the
return value while having already minted the key), tier 0 cannot withdraw *with a funded
claim* (so the refusal is the gate, not insolvency), tier 1 does both, the unconfigured-rail
`None` stays distinguishable from the unverified `Forbidden`, and a revenue payout — funded
end to end by two settled user withdrawals' retained fees — is never gated.
`piggybank/core/tests/relay_recovery.rs`
proves a parked event lands in the distinct `parked_at` state (never marked dispatched),
stays queryable, and is surfaced by `Reconciliation::scan`; that `Reaper::sweep` alerts on
a stuck `processing` withdrawal (never auto-voids it) while auto-cancelling an abandoned
`queued` one; and that an unparked `Dispatched` event for a failed withdrawal is re-parked
by the broadcast-state guard, never sent. They skip when `DATABASE_URL` is unset or TB is
unreachable. Drive the relay deterministically with `Relay::drain`, and the recovery jobs
with `Reconciliation::scan` / `Reaper::sweep` / `Dispatcher::sweep`.

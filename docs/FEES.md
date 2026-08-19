# Fees — the fund's 2 and 20

Two charges sit on a holding and they answer different questions.

**Management, 2% p.a. by default.** Rent on the capital parked in the product. It accrues
continuously with elapsed time and is charged whether the fund made money or not. Its base
is either the **invested capital** (the position's cost basis — the house default) or the
**market value** (`units × NAV`, the hedge-fund convention). Invested capital is the
default for one reason: it does not swell with a mark the operator posted, so the fee the
operator earns stays independent of the input the operator supplies.

**Performance, 20% of the gain by default.** A share of profit above a **per-investor
high-water mark**, crystallized at the end of a period (annual by default) and again when
an investor redeems. Optionally the gain must first clear a **hurdle** accruing at
`hurdle_bps` p.a. over the mark.

A product with no policy row charges nothing. The fee is opt-in per product, so it can
never appear on a fund whose prospectus did not promise it — and "no policy" is a different
fact from "a policy whose rates are zero", which is why `FeePolicy.configured` exists.

## Why the mark is per investor

A fund-level mark mutualizes the fee. An investor who subscribes after a drawdown rides the
recovery fee-free, while one who subscribed at the top pays on gains that only restored
their own loss. The industry's two fixes are *series accounting* (a new share class per
dealing day) and *equalization* (per-investor credits and debits against one class). This
ledger makes a third road cheap: the position projection already exists per
`(user, service)`, so the mark simply lives on it. Every investor's fee is measured against
their own entry price and nobody subsidises anybody.

An on-chain vault cannot do this — a share token cannot remember who bought when — which is
exactly why the mark lives here and not in a contract.

## Why the fee is taken in units, never in cash

A charge claws back units: `Dr FeeShares / Cr UserShares` on the share ledger. No USDT
moves. Three properties follow, and they are the reason for the design.

1. **No chain fee, ever.** Nothing leaves custody when a fee is charged, so the fund pays no
   gas per investor per period. The manager converts an accumulated unit balance to cash
   *once*, in bulk, through `SettleFeeShares`.
2. **An investor can never be pushed negative.** The cash claim is not touched at all, and
   the clawback is capped by the units actually held — with TigerBeetle's
   `credits_must_not_exceed_debits` flag on `UserShares` as the ledger backstop underneath
   the application cap.
3. **Other investors are untouched.** `SharesOutstanding` does not move, so NAV per unit is
   unchanged. The charge is a transfer *between holders*, not a dilution of everyone —
   which is what makes the per-investor mark honest.

Whatever cannot be collected — the holder's units are locked by a queued redemption, or the
residue falls below one base unit of share — is carried as `fee_debt` and collected on the
next assessment. It is never written off and never becomes a negative balance.

## The elapsed clock, and the obligation it places on everyone else

The management leg is `basis × rate × elapsed`, and **both factors live on the same row**.
That makes every writer of `fund_positions.cost_basis` part of the fee plane whether it
wants to be or not: moving the basis without moving the clock means the next assessment
charges the whole elapsed window on money that arrived at the end of it.

`infrastructure::fee_accrual::carry_accrual` is the discharge of that obligation. Before a
basis moves it computes what the *old* basis earned, carries it into `fee_debt`, and
restarts the clock. Both writers call it — the subscribe projection in the relay, and the
redemption settle.

Resetting the clock without settling would be worse than the bug it fixes: an investor
could top up a dollar a day and the elapsed window would never reach the sweeper's minimum
age, so they would never be charged at all.

The dormant case is the one that made this urgent. The sweeper's queue skips unit-less rows
(`units <> '0'`), so a position that goes to zero has its clock frozen for the whole
dormancy. Without the reset, an investor who exited and came back a year later was billed a
year of management fee on their returning capital.

## Concurrency

An assessment reads its snapshot outside a transaction and writes under the position row
lock. The lock orders two concurrent writes but cannot tell that the second one's inputs
went stale while it waited — so both would charge the same elapsed window. The accrual
clock *is* the version: `advance_position` writes conditionally on it, and a charge whose
clock has moved rolls back whole. Losing that race is a non-event, exactly like a fund with
no policy or a charge that floors to nothing.

This matters because a second core instance is a supported deployment — the relay is a
lock-enforced singleton (`pg_advisory_lock`) precisely because one runs — and the sweeper
has no such lock. It does not need one now.

## What the investor sees

The proto marks `GetFeePolicy` readable by any authenticated user: an investor is entitled
to know what they are paying before they subscribe, not after the first charge. The cabinet
surfaces it in three places.

- **The product page** shows the terms in a `Fees` card beside the unit supply, for holders
  and non-holders alike, and adds the caller's own accrued figures (management, performance
  at today's NAV, carried debt, total, and their high-water mark) when they hold a position.
  The `Value` stat opposite is stated *before* fees, and the card says so.
- **The activity timeline** carries a fifth operation kind. A fee charge is the only line
  item that reduces a holding without the investor doing anything, so leaving it out made
  units simply shrink with nothing to explain it.
- **The operation detail panel** breaks the charge into its two legs, the units taken, and
  the price they were taken at.

## Settlement

`SettleFeeShares` is the one operation in the plane that moves cash. It runs once per period
for a whole fund rather than once per investor — the entire point of collecting in units. It
is Read-First gated on the fund's claim covering the payout and **refuses** when short
rather than queueing: nobody is waiting on it, and a fee that cannot be paid today keeps
accumulating as units at no cost.

## Still open

**Exit crystallization is not wired.** `Trigger::Redemption` exists and is tested, but
nothing calls it: fees accrue on the schedule only. An investor who redeems between
crystallizations therefore leaves without paying the performance fee on the gain they are
realising. `carry_accrual` on the settle path closes the management half of that gap; the
performance half is the remaining work.

## Where the code lives

| Concern | Path |
| --- | --- |
| Arithmetic, pure and I/O-free | `domain/src/fees.rs` |
| Use cases | `piggybank/core/src/application/fees.rs` |
| Postgres adapters, the charge | `piggybank/core/src/infrastructure/fees.rs` |
| Settling the accrual before a basis moves | `piggybank/core/src/infrastructure/fee_accrual.rs` |
| The periodic worker | `piggybank/core/src/infrastructure/fee_sweeper.rs` |
| Schema | `piggybank/core/migrations/0023_fee_policy.sql` |
| Wire contract | `contracts/proto/banking/v1/fees.proto` |
| Integration tests (real PG + TigerBeetle) | `piggybank/core/tests/fee_policy.rs` |

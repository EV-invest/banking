# Runbook — ownership data migration (`piggybank migrate-ownership`)

One-off, operator-run, after the first ownership release (#245, phase 1) is in
production and **before** the contract migration that removes the retired accounts.
Background: [`piggybank/core/PATTERNS.md`](../piggybank/core/PATTERNS.md) §Money plane
and the module docs of `piggybank/core/src/application/migrate_ownership.rs`.

## What it does

Every unit of value must be at a holder — a person's own claim, or units of an
allocation people hold. Before this migration three things are not:

| Legacy account | Where it goes |
| --- | --- |
| `fund` (code 1) — the seed capital claim | moved onto `service:fund`; units of the `fund` allocation minted to the named holders pro rata at NAV 1.00 |
| `fee` (code 40) — retained withdrawal / 2-and-20 / taker fees | moved onto `service:fee`; units of the `fee` allocation minted to the named holders pro rata at NAV 1.00 over **cash + every product's fee class at its dealing NAV** |
| `shares_fee:<product>` — a product's fee class | stays where it is (it is already the `fee` allocation's holding); it is only *priced* into `fee`'s value |
| `shares_company:<product>` — the retired company stake | `--company keep` (default): the run **refuses** while any is non-zero, naming the figure. `--company retire`: burnt (`Dr shares_outstanding / Cr shares_company`, no cash), supply shrinks by exactly the stake |

Per allocation (`fund` first, then `fee`) the cash hand-over and every holder's mint are
**one TigerBeetle linked chain**: they land together or not at all. The mints are posted
by the command itself, not by the relay, and their `unit_issuances` rows are written
already `applied` with the holders' `fund_positions` projections — the only writes in the
system that bypass the outbox, which is admissible because the command is run once, by
hand, after a dry run, with the reconciliation printed at the end. Every id is
deterministic (derived from the holder table), so a second run finds its own work and
reports `already applied`.

There is **no rollback** once the chain has landed. Hence the dry run and the `yes`.

## Input: `holders.json`

```json
{
  "fund": [
    {"user_id": "<banking user uuid>", "share_bps": 8000},
    {"user_id": "<banking user uuid>", "share_bps": 2000}
  ],
  "fee": [
    {"user_id": "<banking user uuid>", "share_bps": 8000},
    {"user_id": "<banking user uuid>", "share_bps": 2000}
  ]
}
```

- `user_id` is the **banking** `users.id` (not the concierge id); every person must exist
  and be active.
- Shares are basis points; each table must add up to exactly `10000`, nobody twice, no
  zero shares.
- Units are `floor(value × bps / 10000)` per holder; the **last holder listed** takes the
  rounding remainder (at most a few base units of 1e-18), so `Σ units == value` exactly
  and NAV is 1.00 to the base unit.
- The table is part of the migration's identity: rerunning with a different table after
  a run is refused (`Conflict`). Keep the file.

## Procedure (spec §4.3)

1. R1 is in production; the pod is up; migration `0044` applied (the `fee` / `fund`
   allocation rows exist). Owners have handed over the holder table and the decision on
   any company stake.
2. **Fresh marks.** Every product whose fee class the `fee` allocation holds must have a
   valuation posted within the last 24 h (`MAX_NAV_AGE_SECS`): the migration prices the
   fee class at the product's dealing NAV and refuses a stale one.
3. **Snapshot before**: `/cabinet/admin/treasury` (the `retired` figures and each
   allocation's line) and the C-0 snapshot. The outbox must be drained (no parked rows,
   nothing pending on `fund` / `fee` — the command refuses in-flight pendings).
4. Copy the table into the pod and **dry-run**. The classifier blocks `kubectl exec` on
   production from a session — the operator runs these by hand:

   ```sh
   kubectl -n apps cp holders.json deploy/ev-banking-piggybank:/tmp/holders.json -c piggybank
   kubectl -n apps exec deploy/ev-banking-piggybank -c piggybank -- \
     piggybank migrate-ownership --holders /tmp/holders.json --dry-run
   ```

   The command uses the pod's own env (`DATABASE_URL`, `TIGERBEETLE_*`) and touches
   neither gRPC nor the relay; the server keeps running beside it. Read the plan: the
   retired balances, `fee`'s value (cash + priced fee classes), every holder's units, the
   company stakes, and `ledger before`. It must match the snapshot.
5. **Run** — the owner confirms with `yes` on stdin (or `--yes` when scripted):

   ```sh
   kubectl -n apps exec -it deploy/ev-banking-piggybank -c piggybank -- \
     piggybank migrate-ownership --holders /tmp/holders.json [--company retire]
   ```

6. Read `=== applied ===`, `=== ledger after ===` and `=== reconciliation ===`. Exit 0
   and `migration complete` means: both retired claims at 0, `Σ holders ==
   SharesOutstanding` for `fee` and `fund`, nothing unheld, cash invariant balanced.
7. Snapshot after: `/cabinet/admin/treasury` shows `fee` and `fund` with their holders,
   `retired` at zero; the reconciliation log stays quiet on the next scans. Only then
   C-9 / R2.

## What is printed

- `=== migrate-ownership: plan ===` — per allocation: the retired balance that moves,
  the allocation's claim before, each priced fee class, the value, the status
  (`PENDING` / `already applied` / `nothing to migrate`) and every holder's units with
  `[minted]` / `[row]` markers; then every company stake and what happens to it.
- `=== ledger before ===` / `=== ledger after ===` — the two retired claims and both
  reserved allocations' ownership pictures (claim, supply, holders, fee classes, NAV).
- `=== applied ===` — per allocation whether the chain was posted and how many rows were
  written or already present; the company stakes retired.
- `=== reconciliation ===` — the server's own scan, once.

## When it exits 1

- **Before anything moved** (a refusal from the plan): the message names the cause — a
  table that does not add up, an unknown or disabled person, a stale mark, a non-zero
  company stake under `keep`, units already outstanding on `fee` / `fund` (someone was
  seated before the migration: the owners decide), in-flight pendings on a retired
  account, or a table that differs from an applied run. Fix the input or the state and
  rerun; nothing was written.
- **After the run, with `FINDING:` lines**: the movements are on the ledger and cannot be
  rolled back. Do not rerun blindly. Compare `ledger after` with the plan, read the
  reconciliation's findings, and treat it as an incident (`docs/RUNBOOK-withdrawals.md`
  style: TB wins, recovery is an operator action).
- **Crash between the chain and the rows** (the process died after `=== applied ===`
  started printing, or the rows are missing while the treasury shows the holders): rerun
  the same command with the same table. The plan reports `already applied on the
  ledger; N rows missing, will be written from the ledger` and writes them with the
  amounts read back from TigerBeetle.

## Known limits

- A legacy outbox row replayed *after* the run (an old fee event still naming `fee`
  code 40) leaves a residue on a retired claim. The reconciliation counts it as
  `retired_claims`; the treasury shows it under `retired`. It is fee income belonging to
  the `fee` holders; moving it is a separate, explicit operator action — the migration's
  cash leg is idempotent and will not move it again.
- The command is removed together with the retired accounts in the contract step (C-9).

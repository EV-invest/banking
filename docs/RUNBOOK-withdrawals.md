# Runbook — stuck / parked withdrawals

Operator recovery for a withdrawal wedged in `processing` with its `Dispatched` outbox
event **parked** (typically `last_error` = "custody rejected: treasury underfunded
on-chain: …"). Background: [`piggybank/core/PATTERNS.md`](../piggybank/core/PATTERNS.md)
§Withdraw + §Relay safety. The **cardinal rule** governs everything below: *never void a
withdrawal once its broadcast may have reached the chain* — that double-pays.

## Operator surfaces

The admin console's **Withdrawals** screen (`/admin/withdrawals`) lists every withdrawal
awaiting action (`queued` / `processing`) with per-row **Dispatch**, **Settle** (mined tx
hash required) and **Fail** (double-pay warning; the hub refuses while a broadcast row
exists). The same RPCs by hand, when the console is down or you are scripting:

```sh
# Mint an admin money token (subject must hold admin/owner in banking):
grpcurl -plaintext -d '{"user_id":"<banking-user-uuid>"}' localhost:50052 banking.v1.AuthService/IssueUserToken
TOKEN=…

grpcurl -plaintext -H "authorization: Bearer $TOKEN" -d '{}' localhost:50051 banking.v1.BalanceService/ListWithdrawalQueue
grpcurl -plaintext -H "authorization: Bearer $TOKEN" -d '{"withdrawal_id":"<id>"}' localhost:50051 banking.v1.BalanceService/DispatchWithdrawal
grpcurl -plaintext -H "authorization: Bearer $TOKEN" -d '{"withdrawal_id":"<id>","tx_ref":"0x…"}' localhost:50051 banking.v1.BalanceService/SettleWithdrawal
grpcurl -plaintext -H "authorization: Bearer $TOKEN" -d '{"withdrawal_id":"<id>","reason":"…"}' localhost:50051 banking.v1.BalanceService/FailWithdrawal
```

(Ports: 50051 = core gRPC, 50052 = auth; adjust to your deployment.)

Since the dispatch gate (`min(TB rail, on-chain treasury)`) and the dispatcher worker
landed, this park is a rare check-then-act residue (the on-chain balance dropped between
the dispatch-time read and the broadcast), not the norm — but the recovery below stays
the same.

A queued or processing row is **not always a user's** withdrawal. A **revenue payout** —
the fund moving its own earned money (retained fees + settled 2-and-20) to an external
wallet — rides this same saga and appears in this same queue; the console labels it *Fund
revenue* in place of an email, and `ListWithdrawalQueue` reports `source: revenue`. The
recovery steps below are identical, with one wording change: **Fail** refunds the `fee`
claim rather than a user, so nobody is waiting on support — but the cardinal rule is
unchanged, because the chain does not care whose money it was.

## Step 1 — prove the broadcast never happened

```sql
SELECT * FROM withdrawal_broadcasts WHERE withdrawal_id = '<withdrawal-id>';
```

- **0 rows** ⇒ nothing was ever signed and no nonce/seqno was burned — every custody
  adapter runs `ensure_treasury_funded` **before** sign/`store_tx`, so an
  underfunded-treasury park guarantees an empty row. Proceed.
- **A row exists** ⇒ **STOP.** The transaction may sit in a mempool or an unreported
  block. Treat as possibly-broadcast: never fail/void; either settle by the on-chain tx
  hash once it confirms, or let the rail's withdrawal watcher decide.

## Step 2 — confirm the park is clean

```sql
SELECT seq, attempts, last_error, parked_at, compensated_at
FROM outbox WHERE event_id = '<parked-dispatched-event-id>';
```

Expect `compensated_at IS NULL`: the `Dispatched` event is single-op (the broadcast), so
nothing half-applied. A non-null `compensated_at` is a different incident (a half-applied
multi-leg event) — reconciliation owns that.

## Step 3 — choose EXACTLY ONE path

### Path R — fail-refund (default: smallest blast radius, unblocks the user now)

1. Call `BalanceService.FailWithdrawal` (`Permission::WithdrawalFail`) — legal from
   `processing`. The relay voids the clearing pending (`CLEARING_VOID_FAIL`), refunding
   the **gross** in full.
2. Leave the parked `Dispatched` event parked **forever**, as forensics. **Never unpark
   it after the fail**: the reservation is now voided, and a broadcast against it would
   be an unbacked outflow breaking `sum(custody) == sum(claims)`. The relay's
   broadcast-state guard (park unless the withdrawal row is `processing`) makes an
   accidental unpark re-park instead of double-paying — but do not lean on it.

### Path C — fund the treasury and complete

1. Fund the rail's treasury hot wallet with ≥ the **net** in USDT plus native gas
   (BNB/TRX/TON). The address is in the boot log ("treasury hot wallet — fund it…"), or
   via the signer's `ProvisionAddress` with the nil user id. Verify the balance
   on-chain.
2. Unpark the `Dispatched` event:

   ```sql
   UPDATE outbox SET parked_at = NULL, last_error = NULL
   WHERE event_id = '<parked-dispatched-event-id>'
     AND dispatched_at IS NULL AND compensated_at IS NULL;
   ```

   The relay re-plans it within its poll: the reserve-applied guard passes, the
   broadcast-state guard passes (the withdrawal is still `processing`), custody
   signs + broadcasts, and the rail's withdrawal watcher auto-settles after N
   confirmations. The withdrawal correctly remains `processing` throughout. Do **not**
   fail it after unparking.

The paths are **mutually exclusive**. The user-facing `CancelWithdrawal` cannot resolve
this (`cancel` is legal only from `Queued`) — correct per the cardinal rule, not a bug.

## Step 4 — verify

- `Reconciliation::scan` is clean: the clearing reserve matches the gross of in-flight
  withdrawals, and (Path C) the parked-row count dropped.
- `GetTreasury.reserved_for_withdrawals` dropped by the withdrawal's gross.
- The reaper's "STUCK processing withdrawal" `error!` stops firing for this id.

---

## TON — a `processing` withdrawal with a kept broadcast row that isn't landing

TON custody signs each withdrawal at a monotonic future seqno and lets the confirmation
watcher re-broadcast it when the chain seqno reaches it (the v4R2 wallet DROPS an
out-of-order seqno instead of queueing it, unlike an EVM mempool). So a `broadcast`'s
first send at a **future** seqno gets toncenter's "external message not accepted" — which
is benign (not this send's turn). Custody therefore **keeps** the `withdrawal_broadcasts`
row and reports success rather than parking, and the watcher drains it in order.

Residual to know about: the v4R2 contract fails the seqno check *before* the signature /
subwallet checks, so at a future seqno a genuinely bad message (should never happen — the
signer we trust produced it) is **indistinguishable** from a benign queued one and is also
kept. It can never land (no double-pay — settle still requires a proven outgoing transfer),
but it will sit in `processing`, and because the row exists the fail-void guard refuses to
auto-void it (Step 1 → "a row exists ⇒ STOP"). To clear such a case:

1. Read the stored send: `SELECT nonce AS seqno, expiration, tx_hash FROM withdrawal_broadcasts WHERE withdrawal_id = '<id>' AND network = 'ton';`
2. Compare the treasury wallet's **current on-chain seqno** to the stored `seqno`. If the
   chain seqno is already **past** the stored one and there is still **no** matching outgoing
   USDT transfer of the net amount (the watcher's settle proof), the message provably never
   applied — it is safe to `FailWithdrawal` (the reservation was never spent). If the chain
   seqno has **not** reached it yet, it is simply still queued — wait (the reaper only
   alerts; it never auto-fails).

## Dead keys (KEK epoch) — diagnostics & rotation

A key sealed under a different `WALLET_KEK` than the signer booted with is **provably
dead**: funds on its address can never be moved. The signer refuses to boot on a
whole-database mismatch (the `kek_sentinel`); per-key casualties surface as:

- the `unseal_failures` reading of the hub's Readiness RPC — a Grafana alert once banking#378 lands (any non-zero = stranded funds);
- `PROVABLY DEAD KEY` `error!` lines in the signer / hub logs;
- the signer's diagnostics RPC (loopback; needs the hub's service token in prod):

```sh
grpcurl -plaintext -H "authorization: Bearer $SERVICE_TOKEN" -d '{}' localhost:50053 signer.v1.SignerService/GetKeyHealth
```

Recovery: rotate the affected user's address so FUTURE deposits are safe (funds already
on the dead address are unrecoverable — do not promise otherwise):

```sh
grpcurl -plaintext -H "authorization: Bearer $TOKEN" -d '{"user_id":"<uuid>","network":"bep20"}' \
  localhost:50051 banking.v1.BalanceService/RotateDepositAddress
```

The signer archives the dead row (`superseded_at`), mints a fresh keypair (unseal-probed
before it is served), and the hub cache refreshes — `GetDepositAddress` serves the new
address immediately. Rotation is **refused for a healthy key**.

## Spend brake — stop or tighten the signer without a restart

The signer's ceilings (`SIGNER_MAX_*`, `piggybank/signer/src/policy.rs`) are loaded once
at boot and are the **unliftable** bound. The `spend_brake` table in the signer's own
database `banking_signer` (migration `piggybank/signer/migrations/0009_spend_brake.sql`)
is the fast lever: **one row** (`id = 1`), read by every signing RPC **before anything
else** — before the Tron freeze, before parsing, before any key lookup — so an `UPDATE`
takes effect on the next request with no redeploy. Each of the six window-level ceilings
becomes `min(env, brake)` for that request; a brake value above the environment's leaves
the environment in force. The brake can only **tighten or halt**, never raise, and it
has **no RPC on purpose**: the hub holds a service token for every signer RPC and is the
adversary the brake exists to stop, and since banking#173 its `evinvest` role has no
`CONNECT` on `banking_signer` at all. The only way in is operator SQL.

**Connect** — on the production host (`evinvest-fallback`), as `postgres` over the unix
socket; no TCP, no password, no RPC:

```sh
sudo -u postgres psql banking_signer
```

**Inspect**

```sql
SELECT * FROM spend_brake;
```

Released posture is `halted = false`, every `max_*` column `NULL` (= the environment's
ceiling applies), `reason NULL`. `updated_at` is set by a `BEFORE UPDATE` trigger — never
type it.

**Stop** — every signature refused until released:

```sql
UPDATE spend_brake SET halted = true, reason = 'incident 2026-09-17: suspected hub compromise';
```

`reason` travels verbatim in the refusal: the hub (the suspect, in this threat model) sees
it, and so do the admin console's outbox `last_error`, Sentry and the nightly dump in R2.
Make it an incident label, never findings, names or indicators.

Every `UPDATE` is recorded by the same trigger in `spend_brake_history` (`by_role`,
`from_addr` — `NULL` means the unix socket, i.e. an operator — and the old/new row as
JSON), and the signer logs `spend brake CHANGED` (`warn`, previous and current state) on
the first signing request that sees a different row — so a release you did not make shows
up as a line, not as the absence of refusals. Read it with
`SELECT changed_at, by_role, from_addr, old_row->>'halted', new_row->>'halted' FROM spend_brake_history ORDER BY id;`.

**Tighten** — units are those of the matching variable: `max_transfer_usdt` and
`max_treasury_usdt_per_hour` in **whole USDT** (like `SIGNER_MAX_TRANSFER_USDT` /
`SIGNER_MAX_TREASURY_USDT_PER_HOUR`); the four `max_native_spend_per_hour_<rail>` columns
in the rail's **base units** — wei (bep20), wei (polygon), SUN (trc20), nanoton (ton) —
like `SIGNER_MAX_NATIVE_SPEND_PER_HOUR_*`. Every column has `CHECK (> 0)`: a zero is what
`halted` is for.

```sql
-- one treasury payout at most 20 USDT, at most 100 USDT per rail per sliding hour
UPDATE spend_brake SET max_transfer_usdt = 20, max_treasury_usdt_per_hour = 100,
  reason = 'incident 2026-09-17: throttling while volumes are checked';

-- BEP20 native window down to 0.1 BNB per (wallet, network) per hour — in wei
UPDATE spend_brake SET max_native_spend_per_hour_bep20 = 100000000000000000;
```

Columns you do not name stay as they were; set one back to `NULL` to hand that ceiling
back to the environment.

**Release**

```sql
UPDATE spend_brake SET halted = false, reason = NULL,
  max_transfer_usdt = NULL, max_treasury_usdt_per_hour = NULL,
  max_native_spend_per_hour_bep20 = NULL, max_native_spend_per_hour_polygon = NULL,
  max_native_spend_per_hour_trc20 = NULL, max_native_spend_per_hour_ton = NULL;
```

**`DELETE` is NOT a release.** The signer cannot tell a deleted row from a broken
database and fails closed on both: a missing row (or any read error) refuses every
signature with `Internal`. If the row is gone, put it back released with
`INSERT INTO spend_brake (id) VALUES (1);` (the seed the migration wrote; a second row is
refused by `CHECK (id = 1)`).

**What you will see.** A halted brake answers every signing RPC with `permission_denied`
"the signer's spend brake is halted (<reason>): no signature is issued until an operator
releases it at the signer's database". The hub treats that exactly like the window refusal
of banking#380: the withdrawal's `Dispatched` event is **parked** in the outbox
(`last_error` "custody rejected: signer: …", `piggybank/core/src/infrastructure/custody.rs`
maps everything but `Unavailable`/`DeadlineExceeded` to `Rejected`), and the sweeper holds
the refused address back with exponential backoff (`sweep: signer refused on the merits …`
at `error!`, `piggybank/core/src/infrastructure/sweep.rs`). A read error / missing row is
`Internal` and parks too. Signer log lines (container `signer` of
`deploy/ev-banking-piggybank`, namespace `apps`):

- `refused: the spend brake is halted` (`warn`, with `reason` and `updated_at`) — per
  refused request;
- `spend brake engaged for this request` (`info`, `brake_*` next to `env_*`) — a ceiling
  column is set and the request ran under the tightened policy;
- `signer spend brake state` at boot — `info` when released, `warn` with `HALTED — every
  signature is refused until an operator releases it` when halted. A signer that cannot
  read the row at boot does not start (`failed to read the signer's spend brake`).

**Not gated:** `ProvisionAddress`, `GetKeyHealth`, `RotateAddress`,
`MigrateAddressToCustodian` — they mint or archive keys and move no money, so
provisioning, the health probe and the dead-key rotation above keep working during a halt.

**After release — parked events do not wake up by themselves.** Each withdrawal the halt
parked stays parked until an operator unparks it: **Unpark** on the admin console's
Outbox screen (`/admin/outbox`, `BalanceService.UnparkEvent` — see
[`piggybank/core/PATTERNS.md`](../piggybank/core/PATTERNS.md) §Relay safety) or the SQL
in Step 3 Path C above, after the same Step 1 check. Held-back sweeps resume on their own
once the backoff expires (an hour at most). Unparking while a ceiling is still tightened
below the payout only re-parks it — release first, then unpark.

**Check after a deploy:** the boot line `signer spend brake state` in the `signer`
container's log shows `halted=false`, and
`sudo -u postgres psql banking_signer -c 'SELECT halted FROM spend_brake'` is `f`.

**Rolling the signer image back past this migration** is not "roll back and leave the
table": `sqlx::migrate!()` refuses to boot a binary that does not know an applied version
(`migration 9 was previously applied but is missing`). Roll forward, or first
`DELETE FROM _sqlx_migrations WHERE version = 9;` as `postgres` over the socket (the
tables may stay — the older binary never reads them), then roll the image back.

## Incident log

### 2026-07 — withdrawal `4974af80-5660-4238-ab1d-59af9c6993b5` (first entry)

2 USDT gross, BEP20, stuck `processing`; parked outbox event
`dd91694c-da4c-4baf-aa13-c2fd5787d584` (kind `withdrawals`, payload type `dispatched`,
`last_error` "custody rejected: treasury underfunded on-chain: 0 < 1000000000000000000
needed"). Root cause: the pre-gate dispatch path read only the TB `wallet:bep20`
accounting balance (which counts un-swept deposit-address funds), while the treasury hot
wallet held 0 USDT on-chain. Resolution: **Path R** (fail-refund) after Step 1 returned
0 rows; `dd91694c…` stays parked as forensics — never unpark it.

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

## Dead keys (KEK epoch) — diagnostics & rotation

A key sealed under a different `WALLET_KEK` than the signer booted with is **provably
dead**: funds on its address can never be moved. The signer refuses to boot on a
whole-database mismatch (the `kek_sentinel`); per-key casualties surface as:

- the **Dead-key signings** counter on the admin Overview (any non-zero = stranded funds);
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

## Incident log

### 2026-07 — withdrawal `4974af80-5660-4238-ab1d-59af9c6993b5` (first entry)

2 USDT gross, BEP20, stuck `processing`; parked outbox event
`dd91694c-da4c-4baf-aa13-c2fd5787d584` (kind `withdrawals`, payload type `dispatched`,
`last_error` "custody rejected: treasury underfunded on-chain: 0 < 1000000000000000000
needed"). Root cause: the pre-gate dispatch path read only the TB `wallet:bep20`
accounting balance (which counts un-swept deposit-address funds), while the treasury hot
wallet held 0 USDT on-chain. Resolution: **Path R** (fail-refund) after Step 1 returned
0 rows; `dd91694c…` stays parked as forensics — never unpark it.

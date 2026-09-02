# Consilium — multi-owner authorization

Two things in this platform must never be one person's decision: paying the fund's
own earned money out, and taking someone's ownership away. Both are gated by a
**consilium** — a quorum of fund owners who each confirm from their mailbox.

This document is the policy. It is written before the code because the failure modes
here are not bugs you notice in staging: they are a payout that left, or an owner who
lost their seat, and neither can be undone.

---

## The two planes

The platform already splits **money** (`banking`) from **identity/ownership**
(`concierge`). The consilium respects that split rather than collapsing it:

| Decision | Authorizing plane | Why there |
| --- | --- | --- |
| Pay fund revenue out on-chain | **banking** (`piggybank-core`) | The money plane must be able to _audit its own authorization_. `docs/ARCHITECTURE.md` explicitly rejects letting a concierge-signed artifact authorize money movement; a consilium verdict is exactly such an artifact, so it is computed, stored and verified in the money plane. |
| Remove a fund owner | **concierge** | Ownership is `Role::Owner`, a concierge-owned fact. Only concierge may mutate it, and the one-way bridge (concierge → banking) must stay one-way. |

Neither plane trusts the other's verdict. What crosses the seam is only:

- **the roster**, which banking already mirrors locally (`0016_user_role.sql`) — so a
  payout never needs a live call to concierge at the moment it is authorized;
- **outbound mail**, which banking asks concierge to send over one new
  service-token-gated RPC, because concierge already owns the only mailer
  (`lettre` + `notification_deliveries` + backoff + daily budget), and standing up a
  second one would violate the "no hand-wired vendor SDK" rule.

That RPC takes a **typed** payload, never rendered HTML. A compromised money plane
must not become a phishing cannon pointed at the owners' mailboxes.

---

## Quorum arithmetic

Let `N` be the number of fund owners at the moment the request is opened.

```
threshold = floor(N / 2) + 1        # strictly more than half of ALL owners
voters    = owners \ { initiator }  # the initiator gets no vote and no token
```

The initiator is counted in the denominator but cannot vote. This is deliberate: if
opening a request removed you from the denominator, opening one would be a way to
lower the bar you have to clear.

| N   | threshold | voters | outcome              |
| --- | --------- | ------ | -------------------- |
| 2   | 2         | 1      | **impossible** — refused at open |
| 3   | 2         | 2      | unanimous peers      |
| 4   | 3         | 3      | unanimous peers      |
| 5   | 3         | 4      | 3 of 4               |
| 6   | 4         | 5      | 4 of 5               |
| 7   | 4         | 6      | 4 of 6               |

**A fund with fewer than 3 owners can never pay itself out.** That is a real
consequence of the chosen rule, not an oversight, and it is why owner removal
enforces a floor (below). The open RPC refuses with an explicit error rather than
creating a request that can never reach quorum.

### Owner removal is a different rule

A removal passes when **either**:

- **(a) the target accepts it** from their own mailbox, or
- **(b) every owner except the target and the initiator votes to remove.**

Path (b) is unanimity over `owners \ {target, initiator}`. With exactly two owners
that set is **empty**, and "everyone in an empty set agreed" is vacuously true — which
would let either owner unilaterally expel the other. Path (b) therefore additionally
requires **at least one eligible peer voter**; with two owners only path (a) exists.

**Floor.** A removal may not leave fewer than **2** owners. Dropping to 2 does suspend
payouts — the table above shows why — but that is a *recoverable* state: two owners can
still admit a third and resume. An earlier draft of this policy set the floor at 3, and
it was wrong: at exactly 3 owners it made a bad actor unremovable forever, because
removal was blocked by the floor and admission (below) needs their agreement. A
recoverable pause beats a permanent deadlock. Resignation (removing yourself) needs no
consilium but is subject to the same floor.

### Admission is governed too — otherwise none of this holds

`SetRole` lets any single owner grant `Role::Owner`. Left alone, that defeats the whole
mechanism: a bad actor mints sock puppets *before* opening an invoice, and then reaches
quorum legitimately. With 2 honest owners and 4 puppets, N = 7, threshold = 4, voters =
6 — the four puppets carry it. Snapshotting the roster at open (pitfall 3) does not help,
because the stuffing happens beforehand.

So granting ownership is itself a consilium, and `SetRole` refuses to grant `owner`
outside it. **Admission passes on unanimity of every owner except the initiator, and
requires at least one such peer** — the same shape as removal path (b). Unanimity, not a
majority, because a minority must never be able to grow itself into a majority.

That closes the loop: no minority can add owners, and no minority can remove them.

**One carve-out, because the rule cannot bootstrap itself.** Admission needs a non-empty
peer set, so it can never seat the *second* owner: a fund of one could never grow, and a
fund of zero could never start. A direct grant is therefore allowed only while the
persisted roster holds **fewer than two** owners, and is refused from two onward — which
is exactly the point at which sock-puppeting would have to begin. `SetRole` refuses both
granting and stripping ownership outside that window; the consilium performs the write
itself, so there is no "an admission exists, therefore grant" check to replay.

**Residual, stated plainly:** whoever controls `ADMIN_SUBJECTS` can use that genesis
window on a fund with fewer than two owners, and can still suspend or disable an owner
at any time. Suspension is a lesser power than seating one — it cannot manufacture a
quorum, only withhold one — but it is not nothing, and it is the reason break-glass
subjects hold no seat in any consilium: the roster is read from the persisted role, never
from the environment-promoted one.

| Fund state | What is possible |
| --- | --- |
| 2 owners | admit a third (both must agree); no payouts; no removal except self-acceptance |
| 3 owners, one bad actor | the two honest owners remove them (peer set is non-empty, unanimity of one), leaving 2; then admit a replacement |
| 3 owners, two colluding | the same move, pointed the other way — they can expel the third and then admit whoever they like. The payout supermajority does not protect a minority against a majority, and nothing here does. The cooling-off period below is what makes it visible. |
| N ≥ 3 | payouts at `floor(N/2)+1`; removal and admission as above |

**Residual, and stated honestly:** a fund of 2 whose owners disagree is stuck until one
of them accepts removal — there is no third party to break the tie, and inventing one
(a platform admin who can overrule owners) would hand that admin the fund. We prefer the
deadlock to the backdoor.

### What the composition actually guarantees — read this before trusting the table

Each rule above behaves exactly as written. Their *composition* is weaker than the
strongest of them, and it would be dishonest to leave that implicit.

The payout rule alone is a supermajority: reaching quorum needs the initiator plus
`threshold` others, i.e. `floor(N/2) + 2` of `N` — unanimity at N=3 and N=4, 4 of 5,
5 of 6, 5 of 7. No minority can clear it. But the roster is reachable by a lesser bar:

1. Owners A, B, C. A and B are a bare majority and **cannot** pay out — at N=3 a payout
   needs both peers, and C would refuse.
2. A opens a removal of C. The peer set is `{B}`, so **one** REMOVE carries it. C is
   expelled, and C never had a vote that mattered.
3. N=2. A admits a puppet with B's agreement. N=3. Repeat until the roster is theirs.
4. A opens a payout. It now passes on the rule as written.

So the operative bound is not "a supermajority of owners" — it is **"any group that can
survive one removal round"**, which at N=3 is a bare 2 of 3. This is majority governance.
Nothing in this document protects a minority of owners against a determined majority,
and no arrangement of quorums can: whoever holds the majority holds the roster.

**What we do instead of pretending otherwise — a cooling-off period.** An executed
admission or removal blocks opening a payout consilium for 48 hours, and voids any
payout consilium already open. That does not stop the capture above; it makes it
*visible before the money can move*. The expelled owner receives their removal mail and
has two days in which no payout can be authorized — time to raise it with a human, or
to act. Capture becomes noticed rather than silent and instant, which is the honest
thing a mechanism can buy here.

The cost is real and accepted: a legitimate roster change also delays a legitimate
payout by two days.

---

## What can go wrong, and what closes it

### Roster

1. **Roster drift mid-vote.** The eligible voter set and `threshold` are snapshotted
   when the request is opened. Votes are additionally re-validated at execution: an
   approver who is no longer an owner does not count. Changing the roster can
   therefore only make approval _harder_, never easier.

   **This is eventual, not instantaneous, and the bound is the bridge.** The money plane
   counts against its own mirror of `users.role`, fed by the asynchronous
   concierge → banking lifecycle pull (`BRIDGE_POLL_SECS`, 5s nominal, unbounded if the
   bridge is wedged). An owner removed in concierge stays countable in banking for that
   lag, and because their vote arrives by emailed token rather than a session, revoking
   sessions does not stop them. Monitor bridge lag; treat a wedged bridge as a
   governance incident, not just a staleness one.
2. **Quorum-lowering.** Kicking owners after opening a payout cannot shrink the
   frozen `threshold`, and the kicked owner's vote is voided by (1).
3. **Roster stuffing.** An owner added after the request opened is not in the
   snapshot and gets no token, so new owners cannot be minted to reach quorum.
4. **The initiator's vote.** Enforced by _never minting them a token_, not by a check
   at submit time. There is no code path that can accept it.

### The emailed link and the code

5. **Mail scanners click links.** Gmail, Outlook SafeLinks and corporate gateways
   issue automatic `GET`s on every URL in a message. So the `GET` is strictly
   read-only: it renders a redacted summary and nothing else. The vote is a `POST`
   carrying the **secret code**, which only a human who opened the message can type.
   This — not two-channel security — is what the code is actually for; it turns a
   scanned link into a deliberate act.
6. **Token leakage** via `Referer`, proxy logs and browser history. The approval pages
   send `Referrer-Policy: no-referrer`, the token is single-use with a 72h TTL, and on
   its own it can only _read_.
7. **Code brute force.** 10 characters from an unambiguous alphabet (Crockford
   base32 minus `I L O U`) ≈ 46 bits. Five failed attempts burn the token and notify
   every owner. The attempt counter is incremented **in the same transaction, before
   the comparison** — otherwise concurrent requests slip past the limit — and it is
   incremented **only for a seat that has not yet answered**. An unconditional
   increment is incompatible with the `CHECK (attempts <= 5)` that backs the ceiling: a
   seat that answered correctly on its fifth try sits at exactly 5 without burning (the
   correct-code path never burns), and the next retry would violate the constraint and
   turn that seat's every future request into a 503.
8. **Timing side channels.** Token and code are compared in constant time, as hashes.
9. **Secrets at rest.** Only SHA-256 of the token and of the code are stored. The
   plaintext code exists in the concierge delivery row until the message is sent, and
   that payload is nulled on success.
10. **Enumeration.** Unknown, expired, spent and wrong-state tokens all produce one
    identical response. A caller cannot tell which they hit.
11. **Replay.** `UNIQUE (request_id, voter_user_id)` plus a one-shot `decided_at`. A
    repeat of the _same_ decision is an idempotent no-op; a _different_ one is refused.

#### One specification for both planes

Both planes implement an emailed-token flow, and they must not be extracted into a
shared crate — the plane split forbids sharing this state, and the rules genuinely
differ (a majority threshold over N tokens here, unanimity over one token there). But
duplicated code with *two* specifications is what bites. So the behaviour is specified
once, here, and each plane's tests assert against this table:

| Situation | What happens |
| --- | --- |
| Reading an invitation | Costs nothing. Never consumes an attempt, in either plane. |
| Correct code | The vote is recorded. The attempt counter is **not** reset — a token that has been guessed at stays closer to burning. |
| Wrong code | Counts an attempt, and answers `INVALID_ARGUMENT` with the remaining count. This is not an enumeration oracle: it is only reachable by someone already holding a valid, live, unspent token. |
| 5th wrong code | The token burns permanently and every owner is notified. The counter is clamped at 5, so it can never exceed the ceiling its own `CHECK` constraint enforces. |
| Token against an already-closed request | Refused **before** an attempt is counted, so a mail scanner cannot spend a human's budget. |
| Repeating the same decision | Idempotent success, and it costs **no** attempt — a seat that has already answered is charged nothing, so a double-click can never walk it to the burn ceiling. |
| A different second decision | Refused; the first answer stands. |
| Unknown / expired / spent / burned token | One identical `NOT_FOUND`, indistinguishable from each other. |

### Binding the vote to what was actually approved

12. **Editing the invoice after approvals.** There is no edit RPC — the request is
    immutable. `payload_hash` is stored at open and re-verified at execution, and the
    email shows the full amount, asset, network and destination address plus the
    hash prefix, so an owner approves a thing they can see. Changing anything means
    cancel and reopen; votes are not carried over.
13. **Truncated addresses.** The destination is rendered in full, monospace. A
    `0x1234…abcd` in an approval email is an invitation to approve the wrong address.

### Executing the money move

14. **TOCTOU on the tally.** The count and the transition to `Approved` happen in one
    Postgres transaction with `SELECT … FOR UPDATE` on the request row.
15. **Double payout.** Execution is idempotent: the withdrawal id is
    `uuid_v5(request_id, "consilium:revenue-payout")`, so a retried execution
    re-creates the same row and the existing saga treats it as a no-op.
    `executed_withdrawal_id` is written once.
16. **Concurrent approved payouts overdrawing revenue.** At most **one** payout
    consilium may be open at a time (partial unique index). This removes the race
    rather than trying to win it. Insufficient revenue at execution is still handled:
    the existing solvency Read-First rejects it, the request lands in
    `ExecutionFailed` with the reason visible to owners, and nothing retries silently.
17. **Stale approvals.** Requests expire after 72h. An expired request can never
    execute, even if a vote arrives late.

### Owner removal

18. **Vacuous unanimity** with two owners — closed by the "at least one peer voter"
    rule above.
19. **The last owners.** The floor of 2 keeps a fund from being emptied of governance
    entirely, while staying recoverable.
20. **Mutual expulsion race.** At most one open removal per target; at execution the
    initiator's own ownership is re-checked, so a removal opened by someone who has
    since lost their seat is void.
21. **Sock-puppet quorum.** Ownership cannot be granted by one owner: `SetRole` refuses
    `owner` outside an admission consilium, which needs unanimity of the other owners.
    Without this every other control here is decorative — a bad actor simply mints the
    majority they need before opening an invoice.

### The live consilium page

22. **The socket is not the source of truth.** It carries a version number, never a
    tally. The client re-fetches the authoritative snapshot when the version moves, so
    a stale or spoofed frame can never render a wrong count.
23. **Authorization at handshake.** The websocket verifies `ev_access` exactly as the
    REST routes do, and closes when the token expires.
24. **No secrets on the wire.** No tokens, no codes, no email addresses.
25. **Multiple replicas.** Correctness never depends on the in-process broadcast: the
    stream also re-reads on an interval, so a second replica cannot go blind.
26. **CSP and deadlines.** The websocket origin is added to `connect-src`, and the
    route is mounted outside the BFF's request-timeout layer.

### There is no way around it

27. **The direct payout RPC.** `BalanceService.RequestRevenuePayout` used to move the
    fund's revenue on ONE Admin/Owner's say-so, gated on `Permission::RevenuePayout` —
    the same permission that merely lets someone *open* a consilium. Any principal who
    could propose a payout could equally well skip the proposal and take the money, so
    the whole mechanism was decorative. The RPC now refuses with `FAILED_PRECONDITION`
    and names `ConsiliumService.OpenRevenuePayout` as the only route.
    `CancelRevenuePayout` is untouched: cancelling refunds, and is not the hazard.
28. **A mechanism that silently does nothing.** Every approval token reaches its owner
    through exactly one route — the `consilium_mail` queue drained into concierge. On a
    build where that seam is not compiled in, a consilium would open, mail nobody, and
    expire 72h later having been unvotable from the first instant, while every screen
    reported it healthy. Opening now **refuses** when no mailer is wired. Refusing at
    open rather than at boot is deliberate: refusing at boot would take the whole money
    plane down over a feature that is off by default and orthogonal to all of it.

---

## Audit

Every vote records who, when, from which IP and user agent, and against which
`payload_hash`. Nothing is deleted: a rejected, expired or failed request stays
readable. The domain-event log carries the same facts, so the ledger and the
governance record can be reconciled after the fact.

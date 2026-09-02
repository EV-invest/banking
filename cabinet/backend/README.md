# cabinet-backend

The cabinet's **BFF** (backend-for-frontend): a standalone, **stateless** HTTP service that
is the cabinet's egress boundary. It proxies the browser's same-origin `/api/*` JSON
requests to two gRPC planes:

- **concierge** (identity) — `UserDirectory` `GetMe`/`UpdateProfile`, the directory/platform
  admin RPCs, notifications, and the ownership plane (owners, removals, the live feed).
- **piggybank** (money) — `WalletService`, `FundsService`, `FeesService`, `AllocationsService`,
  `BalanceService`, `ConsiliumService`, `HealthService`.

## Auth is shell-owned

The BFF **runs no OAuth and holds no session**. The concierge auth web surface — reached
through the conductor's `/api/auth/*` on the shared origin — signs the user in and sets the
zone-shared `ev_access` JWT cookie. This service only **verifies** that cookie, locally,
against the concierge JWKS (cached from the plane's public `Jwks` RPC, so there is no
per-request round trip; it fails closed until the plane publishes keys). That verified
cookie IS the request's credential.

There is no session store, no opaque session cookie, and no `oauth.rs`. The one piece of
server-side state left is `session.rs`'s per-user **cache** of banking money tokens, which
a lost entry simply re-mints.

Cookies read (never set): `ev_access` (the access JWT) and `ev_csrf` (the readable
double-submit token, echoed in `x-ev-csrf` on every mutation). Both are `__Host-`-prefixed
in production.

## Layout

| Module | Role |
| ------ | ---- |
| `config.rs` | the `ev::settings!` env surface — the deploy contract, pinned by tests |
| `state.rs` | `AppState` + `Grpc` (lazy channels + typed client calls to both planes) |
| `session.rs` | the per-user banking money-token cache + single-flight refresh |
| `cookies.rs` | the cookie names the BFF reads (must match the shell's) |
| `governance.rs` | the concierge ownership-plane seam — see the pin note below |
| `dto.rs` | browser-facing JSON DTOs (snake_case; 64-bit values as strings) |
| `error.rs` | gRPC status → HTTP status + `{ "error": … }` body |
| `routes/` | one handler per endpoint: `identity`, `money`, `admin`, `notifications`, `platform`, `system`, `consilium`, `approval`, `governance_ws` |

## The governance surface

Two things must never be one person's decision — paying the fund's own revenue out, and
taking an owner's seat away. Both are gated by a **consilium**; `docs/CONSILIUM.md` is the
policy and the threat model.

| Route | Plane | Token |
| ----- | ----- | ----- |
| `GET /api/consilium`, `GET /api/consilium/{id}` | money | banking |
| `POST /api/consilium/revenue-payout`, `POST /api/consilium/{id}/cancel` | money | banking |
| `GET /api/owners`, `POST /api/owners/resign` | ownership | concierge |
| `GET`/`POST /api/owners/removals`, `POST /api/owners/removals/{id}/vote`, `…/cancel` | ownership | concierge |
| `GET`/`POST /api/owners/admissions`, `POST /api/owners/admissions/{id}/vote`, `…/cancel` | ownership | concierge |
| `GET /api/owners/consilium/ws` | ownership | concierge |

The split is architectural: the money plane must be able to audit its own authorization, so
its tally is computed against the owner roster it already mirrors; ownership is a
concierge-owned fact, so only that plane may mutate it. Each handler forwards its own
plane's token and never the other's.

**Admission is governed too**, and it is why the rest holds: granting a seat needs
unanimity of every other owner, and it is the ONLY way `Role::Owner` is granted — the
directory's `SetRole` refuses it outside an executed admission. Without that, a bad actor
would mint sock puppets *before* opening an invoice and then reach quorum legitimately,
which snapshotting the roster cannot prevent because the stuffing already happened.
Admission and removal share one governance revision, so the socket follows both. Note the
two votes have deliberately different vocabularies — `admit`/`reject` against
`remove`/`keep` — because a page that rendered the wrong verb would still submit.

**The websocket** bridges `GovernanceService.WatchGovernance` to the browser — one revision covers removals and
admissions together, so a single subscription follows the whole ownership surface. A frame
carries a revision and a timestamp — never a tally, never a secret — and the client
refetches the authoritative snapshot when the revision moves. It verifies `ev_access` at
the handshake exactly as the REST routes do, checks the handshake `Origin` against
`CABINET_WS_ORIGIN` (a websocket handshake is exempt from CORS but still carries cookies),
and closes itself when the token expires. It is mounted **outside** the router's
request-deadline layer, which exists to kill a wedged request and would otherwise kill this
every 15 seconds.

**`/api/approval/{payout,removal}/{token}`** is the surface an emailed owner reaches. It
carries no session and requires none — the emailed token is the credential — so it is the
one part of this service with no cookie and no CSRF check. The `GET` is strictly read-only
because mail scanners fetch every URL in a message; the vote is a `POST` carrying the
secret code from the same mail. Every response sends `Referrer-Policy: no-referrer` and
`Cache-Control: no-store`, and every unusable token — unknown, expired, spent, burned,
wrong-state — produces one identical 404, so the endpoint cannot be used to probe which is
which. The POST is bounded by a small in-process per-IP limiter; the real anti-brute-force
bound is the plane's five-attempt token burn.

## Run

`nix run .#cabinet-backend`. It needs the piggybank hub (`nix run .#piggybank`, or `.#dev`);
identity flows additionally need the concierge runner, started from the sibling `concierge`
repo, and a signed-in browser session from the shell's auth surface. Every port comes from
the flake's `ports` attrset; secrets and per-machine overrides live in `.env.example` (copy
to `.env`).

> **Network segmentation.** `CABINET_BACKEND_BIND` stays loopback-only because the BFF's
> request-auth is a cookie: it must be reached **only** through the frontend's same-origin
> `/api/*` reverse proxy. Widen the bind (`0.0.0.0`) only behind an upstream firewall that
> keeps `/api/*` off any public interface — see
> [`docs/ARCHITECTURE.md`](../../../docs/ARCHITECTURE.md).

> **Two token pairs (cross-plane trust).** The two planes sign under separate issuers and
> distinct audiences (concierge `aud=concierge`, banking `aud=banking-core`). The BFF
> forwards each plane its **own** token and never the other's, so a leaked identity token
> cannot move money. The banking pair is **exchange-based**: for a verified JWT subject the
> BFF calls banking `AuthService.IssueUserToken` — authenticated by the shared
> `BANKING_ISSUANCE_TOKEN`, *not* by piggybank trusting concierge's issuer — and banking
> mints an `aud=banking-core` pair for the bridge-mirrored user. If the bridge has not
> mirrored a brand-new user yet, the money routes surface `NotConfigured` (503) until a
> later request re-mints. Cross-plane revocation: a concierge `SUSPENDED` freezes money ops
> immediately (per-op gate); a `SESSIONS_REVOKED` invalidates the money family within the
> banking access TTL (enforced at refresh).

## Checking the deploy contract

`cabinet-backend --print-required-vars[=PROFILE]` prints the variables a profile must
provide. The gitops preflight runs it against the built image and diffs it with the cluster
Secret's keys, so a missing variable is caught before the rollout rather than as a
`CrashLoopBackOff` after it. Production additionally requires `SENTRY_DSN` (errors must
reach someone) and `CABINET_WS_ORIGIN` (the socket's cross-site guard must not fail open).

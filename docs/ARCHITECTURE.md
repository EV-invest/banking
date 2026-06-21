# Architecture

`banking` is the **central public hub** of the EV banking platform. It owns the money,
the identities, the contracts, and the client shell; every other service lives in
its own repo and talks **only to the hub, over gRPC**. Services never talk to each
other — not directly, not even via a hub round trip.

This repo began as a **scaffold**; the **money plane** is now implemented end to end —
the `balance`, `withdrawals`, `subscriptions`, and `redemptions` bounded contexts, the
TigerBeetle `Ledger` gateway (cash + a **fund-units / service-currency** ledger), and the
transactional outbox + single-worker saga relay (see
[`piggybank/core/PATTERNS.md`](../piggybank/core/PATTERNS.md)). Clients invest by buying
fund **units** priced at NAV (value = units × NAV); the remaining domain/application areas
stay documented placeholders until a feature explicitly asks.

## Topology

```
   ┌───────────────────────────── banking (this repo) ────────────────────────────┐
   │  clients/cabinet (Next.js BFF) ──gRPC──▶  piggybank (one process)            │
   │      ▲  composes <mfe-*>                 ├─ core task  : gRPC services       │
   │      │  custom elements                  │              (balance/users/…)    │
   │                                          └─ auth task  : issuance gRPC       │
   │                                           core ◀─Authorizer channel─ auth    │
   │                                           Postgres (control) · TigerBeetle   │
   │                                           (money) · Redis (central auth)     │
   └──────────────────────────────────────────────────────────────────────────────┘
   browser ─HTTP─▶ clients/cabinet BFF ─gRPC─▶ piggybank core
   other service repos (separate): own logic+allocations ─gRPC (evbanking_contracts)─▶ piggybank;
     verify client tokens locally via evbanking_auth; own microfrontends mount into clients/cabinet.
```

## Cargo workspace (crate graph)

```
domain            → ev (architecture feature)          [wasm-safe]
evbanking_contracts  → (tonic-build over proto/)
evbanking_auth       → evbanking_contracts
piggybank-core    → domain, evbanking_contracts, evbanking_auth
```

| Crate                                | Role                                                                                                                                            | wasm-safe |
| ------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------- | --------- |
| `domain/`                            | Pure DDD types over `ev::architecture`; the bounded contexts (`balance`, `users`, `withdrawals`, `subscriptions`, `redemptions`, `auth`). Source of truth across platform. | **yes**   |
| `evbanking_contracts` (`contracts/`) | gRPC wire contracts: tonic client+server stubs from `proto/`. Other repos import it for the client stubs.                                       | no        |
| `evbanking_auth` (`piggybank/auth/`) | The auth **service** (issuance gRPC + in-process `Authorizer` channel) **and** the shared verification flow (JWKS verify + interceptor).        | no        |
| `piggybank-core` (`piggybank/core/`) | The hub server: composition root that runs the core gRPC services and the auth service as in-process tasks; data-plane services + infra.        | no        |

`domain` never depends on an adapter, and the wasm-unsafe `evbanking_auth` is never a
dependency of `domain` — so `domain` stays wasm-safe for service frontends.
`evbanking_auth` depends on `evbanking_contracts` (it serves the `AuthService` routes), so
`contracts` must never depend back on it.

## Bounded contexts

| Context       | Owns                                                    | Authoritative store        |
| ------------- | ------------------------------------------------------- | -------------------------- |
| `balance`       | the bank's own capital (company money) + the treasury/deposit reads | TigerBeetle              |
| `users`         | investor accounts and their investments                 | Postgres + TigerBeetle     |
| `withdrawals`   | user withdrawals to chain (accept-and-queue saga)       | Postgres + TigerBeetle     |
| `subscriptions` | buying fund units at NAV (the service currency, mint)   | Postgres + TigerBeetle     |
| `redemptions`   | redeeming fund units to cash (accept-and-queue saga)    | Postgres + TigerBeetle     |
| `auth`          | identities + token issuance/verification                | Postgres + Redis (central) |

## Contracts pipeline

`contracts/proto/banking/v1/*.proto` is the single source of truth, and there is a
**single codegen path**: `contracts/build.rs` runs `tonic-build` (client **and**
server) on every `cargo build`. `piggybank-core` uses the data-plane servers,
`evbanking_auth` uses the `AuthService` server (issuance routes), and other repos use
the clients. `evbanking_contracts` vendors the proto, so a downstream repo adds it as
one git dependency (plus `evbanking_auth` for the verification flow) with no protoc
toolchain.

The hub's only TS surface (`cabinet`'s BFF) is a thin gRPC proxy, so it needs no
TypeScript codegen: it reads the same `contracts/proto` at runtime with
`@grpc/proto-loader` (`clients/cabinet/shared/bff/grpc.ts`). No buf, no second
toolchain — tonic + tonic-build do everything.

## Auth

The model is **stateless verification everywhere, state in exactly one place** —
with the hub's own core ↔ auth check done **in-process**, not over the wire.

**Inside the hub (`piggybank`).** `core` and `auth` run as two tasks in one
process (spawned by `piggybank-core`'s composition root). The `auth` task
(`evbanking_auth`) owns the signing keys / JWKS / Google client / refresh store,
serves its **own issuance gRPC routes** (`auth_grpc_addr`, e.g. `:50052` —
`Exchange`/`Refresh`/`Logout`/`Jwks`), and exchanges two cloneable in-process
channel handles with `core`, both crossing a task boundary, **never the network**:

- [`Authorizer`] (core → auth): `core` mounts the async [`grpc_auth_layer`] on each
  data service; the layer calls `authorizer.authorize(token)` to verify a request
  and inject the `Claims`. Auth holds the keys; core never does.
- [`Provisioner`] (auth → core): after a verified Google sign-in, `auth` asks
  `core` to upsert the `users` aggregate (core owns Postgres, the only writer) and
  returns the hub user id + `token_version` to stamp on the minted token.

**Issuance.** The auth service mints the hub's **own** short-TTL (5–15 min)
asymmetric JWTs (EdDSA/Ed25519) after Google OAuth2 (code + PKCE) — it verifies
Google's `id_token` locally and **discards it**, never forwarding it inward — with
`sub` = the hub user id (never Google's `sub`). It publishes verification keys via
the **`Jwks` gRPC RPC** (the hub speaks only gRPC — there is no HTTP `.well-known`).

**Token separation.** The two **signed JWT** directions carry a `typ`
(`access`/`service`) and a **distinct `aud`**: client access → `banking-core`,
inter-service → `banking-services`. A verifier states the issuer, the accepted
**audience set**, and the accepted **`typ` set**, so http clients and grpc services
are cryptographically kept apart. **Refresh tokens are not JWTs** — they are opaque,
rotated, server-side handles (which is what enables reuse detection), so they can't
hit the data plane at all and carry no `aud`/`typ`.

**External services (separate processes).** They can't share the in-process
channel, so they build a [`Verifier`] ([`evbanking_auth`]) that caches the hub's
JWKS (refreshed via the `Jwks` RPC, and on an unknown-`kid` miss) and verify the
hub's JWTs **locally**: **no per-request round trip, no per-service token storage.**
A per-service Redis denylist is an anti-pattern — it reintroduces the round trip
JWTs exist to avoid. They mount the same [`grpc_auth_layer`], authenticate their own
onward calls with a [`ServiceTokenSource`], and depend on `evbanking_auth` (the
flow) + `evbanking_contracts` (the stubs). Downstream adoption guide:
[`piggybank/auth/README.md`](../piggybank/auth/README.md).

**State.** The **only** stateful auth store is one **central** Redis (`REDIS_URL`):
refresh-token rotation + reuse detection, optional `jti` revocation. A per-user
`token_version` claim gives coarse "revoke all" without fleet state — enforced at
**refresh** (the auth service re-reads the authoritative version over the
`Provisioner` channel and refuses to mint), while stateless downstream verifiers
rely on the short access TTL. _Slice note:_ the refresh store currently runs
in-process (single-instance/dev), with the central Redis as the documented
production backing.

**Browser.** The BFF token-handler pattern: `clients/cabinet` is the OAuth confidential
client, holds tokens server-side, and gives the browser only a
`__Host-`/`HttpOnly`/`SameSite` cookie + CSRF defense, scoped to a real apex domain.
Implemented as `clients/cabinet/app/api/auth/{login,callback,logout,session}`
(over `clients/cabinet/shared/auth/*`): `login` mints PKCE+state+nonce and redirects
to Google; `callback` validates `state` against the HttpOnly tx cookie and calls
`AuthService.Exchange`; the hub's JWTs live in a server-side session (in-process for
now, `SESSION_REDIS_URL` in production — distinct from the auth refresh Redis), the
browser holds only the opaque session id + a readable CSRF cookie (double-submit on
`logout`).

**Inter-service.** mTLS + short-lived service JWTs (same stateless verify path,
distinct `aud`); graduate to SPIFFE/SPIRE only at platform scale.

[`Authorizer`]: ../piggybank/auth/src/authorizer.rs
[`Provisioner`]: ../piggybank/auth/src/provisioner.rs
[`Verifier`]: ../piggybank/auth/src/verifier.rs
[`grpc_auth_layer`]: ../piggybank/auth/src/interceptor.rs
[`ServiceTokenSource`]: ../piggybank/auth/src/service_token.rs
[`evbanking_auth`]: ../piggybank/auth

## Microfrontends

The host (`clients/cabinet`, Next.js 16 App Router) composes microfrontends from
other repos at runtime. A microfrontend can be a **whole page or an inline
component**, and may be React or Rust/WASM.

**Universal contract — a custom element.** Every microfrontend ships one
self-registering ESM bundle that calls `customElements.define('mfe-<team>-<name>',
…)`. The host renders it with [`<RemoteElement>`](../clients/cabinet/shared/mfe/RemoteElement.tsx):
load the bundle by URL → `customElements.whenDefined(tag)` → render `<tag>`,
mapping props to attributes/properties and CustomEvents to callbacks. The boundary
is identical for React, Dioxus, and Leptos, so `cabinet` treats every microfrontend
the same. **Light DOM only** — Tailwind v4 `@property` tokens break inside shadow
roots, and global tokens/uikit must cascade in.

- **Registry.** `cabinet` resolves a logical name → `{tag, scriptUrl, kind}` from a
  per-env registry (`clients/cabinet/mfe-registry.json`, served at
  `/api/mfe-registry`). Independent deploys land by editing the registry, not
  rebuilding `cabinet`. Tags are globally unique and versioned (the custom-element
  registry is global).
- **Page-level** = the same element mounted at a route
  (`clients/cabinet/app/(mfe)/[service]/[[...slug]]`); `cabinet` keeps its chrome.
- **React producer** (other repos): wrap a component as a custom element with
  `@r2wc/react-to-web-component` directly — the hub ships no producer SDK (its
  only TS is `cabinet`). _Optional_ optimization for React-to-React widgets: Module
  Federation 2.0 **runtime** (`@module-federation/runtime` + `bridge-react`) to
  share one React instance — never `@module-federation/nextjs-mf`
  (App-Router-unsupported, sunsetting).
- **Rust/WASM producer** (other repos): Dioxus 0.7 mounts via
  `dioxus-web` `Config::rootelement(Element)` into the custom element (don't use
  `dioxus-web-component` yet — it pins Dioxus 0.6); Leptos mounts via
  `mount_to(HtmlElement, …)`. CSR-only, light DOM, `wasm-bindgen =0.2.118`.
  _Open item:_ prove multiple independent Dioxus instances per page before relying
  on it.
- **Rejected:** Next.js Multi-Zones as the primary mechanism (path-only; can't
  embed a widget). It may return later only for standalone legacy sub-sites.

**BFF (orthogonal, server-side).** `cabinet` route handlers proxy browser HTTP to the
hub's tonic backend with `@grpc/grpc-js` + `@grpc/proto-loader` (it reads
`contracts/proto` at runtime — no TS codegen). No microfrontend talks to the
backend directly — `cabinet` is the single auth/egress boundary. WASM MFEs call
`cabinet`'s same-origin BFF over `fetch` (the backend's `tonic-web` layer also allows
direct gRPC-Web when latency demands it).

## Event sourcing + CQRS

Two consistency boundaries, never joined in one transaction:

- **TigerBeetle = data plane** — authoritative money + fund units (balances,
  transfers). Never bookkept a second time in Postgres.
- **Postgres = control plane** — metadata, the UUID↔u128 id-mapping, the domain
  event log + transactional outbox, and CQRS read projections.

**Write path (Write-Last, Read-First).** A command opens one `PgUnitOfWork`
(single Postgres transaction), mutates aggregates, and drains their `EmitsEvents`
into the event log + `outbox` in that same transaction (the only ACID point).
The [outbox relay](../piggybank/core/src/infrastructure/relay.rs) then publishes events to
projections and issues TigerBeetle transfers — money written **last**, after the
Postgres commit; existence checks read TigerBeetle **first**. Cross-boundary moves
are sagas over TigerBeetle two-phase (pending) transfers. Delivery is
at-least-once, so consumers are idempotent (deterministic TB transfer ids; upsert
projections by event id). A reconciliation job compares Postgres projections to
authoritative TB balances; TB always wins.

This matches the `ev::architecture` kernel (`EmitsEvents`/`EventEnvelope`, `Reader`
= CQRS read port, `Gateway` forbidden from `UnitOfWork`). We do **not** adopt a
full event-sourcing framework: `cqrs-es`/`postgres-es` require `sqlx 0.8` (we pin
`0.9`), and a ledger is already an immutable audit log — event-sourcing the same
facts in Postgres would double-bookkeep.

**Money model.** Cash lives in TigerBeetle on one USDT ledger, in **two layers**:
**treasury/custody** (debit-normal wallets, **per rail**) and **claims** (credit-normal
`fund`/`user`/`service`/`fee`/`clearing`, **network-agnostic** — one fungible balance per
party). The invariant is **global**: `sum(custody) == sum(claims)`; a deposit is a single
`Dr wallet:<net> / Cr claim` transfer (no external account). Network lives only at the
custody + transaction edges. Per-rail liquidity is a treasury concern: a withdrawal on a
short rail is **accepted and queued** (reserved against a `clearing` account, decoupled
from any rail) until the treasury tops the rail up. Non-negativity is enforced by TB
account flags.

The **service currency** — fund **units** — lives on a separate TigerBeetle ledger
(independent of cash; the two can't imbalance each other). A client subscribes cash and
receives units priced at **NAV** (derived from an operator-posted AUM); value = units ×
NAV, so profit is a rising NAV. Redemption is the same accept-and-queue shape as withdrawal
(reserve a pending burn, price + pay the cash at settle-time NAV when the fund is liquid).
Saga moves are idempotent by a stable `event_id` (deterministic TB transfer ids;
reservations are two-phase pending with `timeout = 0`). Full chart of accounts, the
fund-shares/NAV model, the queued withdrawal + redemption sagas, idempotency, and the
authorization matrix: [`piggybank/core/PATTERNS.md`](../piggybank/core/PATTERNS.md).

## Run matrix

| `nix run .#`          | What                                                 | Port                          |
| --------------------- | ---------------------------------------------------- | ----------------------------- |
| `dev`                 | postgres + tigerbeetle + redis + piggybank + cabinet | —                             |
| `piggybank`           | hub server: core gRPC + auth tasks (tonic-web)       | `:50051` core / `:50052` auth |
| `cabinet`             | Next.js host shell + BFF                             | `:3000`                       |
| `db` / `tb` / `redis` | local Postgres / TigerBeetle / Redis                 | `:5432` / `:3033` / `:6379`   |

Control-plane migrations live in `piggybank/core/migrations/` and are **applied by
the hub on boot** (idempotent). Author new ones with the sqlx CLI (in the dev shell),
never by hand: `sqlx migrate add --source piggybank/core/migrations --sequential <name>`.

See [`flake.nix`](../flake.nix) for the apps and dev shell, and per-area READMEs
(e.g. [`clients/cabinet/README.md`](../clients/cabinet/README.md)) for details.

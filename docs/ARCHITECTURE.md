# Architecture

`fund` is the **central public hub** of the EV fund platform. It owns the money,
the identities, the contracts, and the client shell; every other service lives in
its own repo and talks **only to the hub, over gRPC**. Services never talk to each
other — not directly, not even via a hub round trip.

This repo is a **scaffold**: structure, contracts, and build wiring are in place;
the domain/application/auth layers are documented placeholders with no business
logic or DB migrations until a feature explicitly asks.

## Topology

```
   ┌───────────────────────────── fund (this repo) ─────────────────────────────┐
   │  clients/core (Next.js BFF) ──gRPC──▶  piggybank (one process)              │
   │      ▲  composes <mfe-*>                 ├─ core task  : gRPC services       │
   │      │  custom elements                  │              (balance/users/…)    │
   │  clients/landing (Next.js)               └─ auth task  : issuance gRPC       │
   │                                           core ◀─Authorizer channel─ auth    │
   │                                           Postgres (control) · TigerBeetle   │
   │                                           (money) · Redis (central auth)     │
   └─────────────────────────────────────────────────────────────────────────────┘
   browser ─HTTP─▶ clients/core BFF ─gRPC─▶ piggybank core
   other service repos (separate): own logic+allocations ─gRPC (evfund_contracts)─▶ piggybank;
     verify client tokens locally via evfund_auth; own microfrontends mount into clients/core.
```

## Cargo workspace (crate graph)

```
domain            → ev (architecture feature)          [wasm-safe]
evfund_contracts  → (tonic-build over proto/)
evfund_auth       → evfund_contracts
piggybank-core    → domain, evfund_contracts, evfund_auth
```

| Crate                                | Role                                                                                                                                            | wasm-safe |
| ------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------- | --------- |
| `domain/`                            | Pure DDD types over `ev::architecture`; the four bounded contexts (`balance`, `users`, `allocations`, `auth`). Source of truth across platform. | **yes**   |
| `evfund_contracts` (`contracts/`)    | gRPC wire contracts: tonic client+server stubs from `proto/`. Other repos import it for the client stubs.                                       | no        |
| `evfund_auth` (`piggybank/auth/`)    | The auth **service** (issuance gRPC + in-process `Authorizer` channel) **and** the shared verification flow (JWKS verify + interceptor).        | no        |
| `piggybank-core` (`piggybank/core/`) | The hub server: composition root that runs the core gRPC services and the auth service as in-process tasks; data-plane services + infra.        | no        |

`domain` never depends on an adapter, and the wasm-unsafe `evfund_auth` is never a
dependency of `domain` — so `domain` stays wasm-safe for service frontends.
`evfund_auth` depends on `evfund_contracts` (it serves the `AuthService` routes), so
`contracts` must never depend back on it.

## Bounded contexts

| Context       | Owns                                                    | Authoritative store        |
| ------------- | ------------------------------------------------------- | -------------------------- |
| `balance`     | the fund's own capital (company money)                  | TigerBeetle                |
| `users`       | investor accounts and their investments                 | Postgres + TigerBeetle     |
| `allocations` | distribution of capital inside the fund and to services | TigerBeetle (sagas)        |
| `auth`        | identities + token issuance/verification                | Postgres + Redis (central) |
| `landing`     | the public marketing site                               | — (`clients/landing`)      |

## Contracts pipeline

`contracts/proto/fund/v1/*.proto` is the single source of truth, and there is a
**single codegen path**: `contracts/build.rs` runs `tonic-build` (client **and**
server) on every `cargo build`. `piggybank-core` uses the data-plane servers,
`evfund_auth` uses the `AuthService` server (issuance routes), and other repos use
the clients. `evfund_contracts` vendors the proto, so a downstream repo adds it as
one git dependency (plus `evfund_auth` for the verification flow) with no protoc
toolchain.

The hub's only TS surface (`core`'s BFF) is a thin gRPC proxy, so it needs no
TypeScript codegen: it reads the same `contracts/proto` at runtime with
`@grpc/proto-loader` (`clients/core/shared/bff/grpc.ts`). No buf, no second
toolchain — tonic + tonic-build do everything.

## Auth

The model is **stateless verification everywhere, state in exactly one place** —
with the hub's own core ↔ auth check done **in-process**, not over the wire.

**Inside the hub (`piggybank`).** `core` and `auth` run as two tasks in one
process (spawned by `piggybank-core`'s composition root). The `auth` task
(`evfund_auth`) owns the signing keys / JWKS / refresh store, serves its **own
issuance gRPC routes** (`auth_grpc_addr`, e.g. `:50052`) — exchanging a client
login for the hub's JWT, refresh, JWKS — and hands `core` an [`Authorizer`]: a
cloneable handle over an in-process channel. `core`'s gRPC interceptor authorizes
each request by calling `authorizer.authorize(token)`, which round-trips to the
auth task **across the task boundary, never the network**. Auth gives its instance
to core; core never holds key material.

**Issuance.** The auth service mints the hub's **own** short-TTL (5–15 min)
asymmetric JWTs (EdDSA/RS256) after Google OAuth2 (code + PKCE) — it never forwards
Google's token inward — and publishes JWKS.

**External services (separate processes).** They can't share the in-process
channel, so they verify the hub's JWTs **locally** against cached JWKS via
`evfund_auth`'s interceptor + `verify_token`: **no per-request round trip, no
per-service token storage.** A per-service Redis denylist is an anti-pattern — it
reintroduces the round trip JWTs exist to avoid. They depend on `evfund_auth` (the
flow) and `evfund_contracts` (the stubs).

**State.** The **only** stateful auth store is one **central** Redis (`REDIS_URL`):
refresh-token rotation + reuse detection, optional `jti` revocation. A per-user
`token_version` claim gives coarse "revoke all" without fleet state.

**Browser.** The BFF token-handler pattern: `clients/core` is the OAuth confidential
client, holds tokens server-side, and gives the browser only a
`__Host-`/`HttpOnly`/`SameSite` cookie + CSRF defense, scoped to a real apex domain.

**Inter-service.** mTLS + short-lived service JWTs (same stateless verify path,
distinct `aud`); graduate to SPIFFE/SPIRE only at platform scale.

[`Authorizer`]: ../piggybank/auth/src/authorizer.rs

## Microfrontends

The host (`clients/core`, Next.js 16 App Router) composes microfrontends from
other repos at runtime. A microfrontend can be a **whole page or an inline
component**, and may be React or Rust/WASM.

**Universal contract — a custom element.** Every microfrontend ships one
self-registering ESM bundle that calls `customElements.define('mfe-<team>-<name>',
…)`. The host renders it with [`<RemoteElement>`](../clients/core/shared/mfe/RemoteElement.tsx):
load the bundle by URL → `customElements.whenDefined(tag)` → render `<tag>`,
mapping props to attributes/properties and CustomEvents to callbacks. The boundary
is identical for React, Dioxus, and Leptos, so `core` treats every microfrontend
the same. **Light DOM only** — Tailwind v4 `@property` tokens break inside shadow
roots, and global tokens/uikit must cascade in.

- **Registry.** `core` resolves a logical name → `{tag, scriptUrl, kind}` from a
  per-env registry (`clients/core/mfe-registry.json`, served at
  `/api/mfe-registry`). Independent deploys land by editing the registry, not
  rebuilding `core`. Tags are globally unique and versioned (the custom-element
  registry is global).
- **Page-level** = the same element mounted at a route
  (`clients/core/app/(mfe)/[service]/[[...slug]]`); `core` keeps its chrome.
- **React producer** (other repos): wrap a component as a custom element with
  `@r2wc/react-to-web-component` directly — the hub ships no producer SDK (its
  only TS is `core`). _Optional_ optimization for React-to-React widgets: Module
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

**BFF (orthogonal, server-side).** `core` route handlers proxy browser HTTP to the
hub's tonic backend with `@grpc/grpc-js` + `@grpc/proto-loader` (it reads
`contracts/proto` at runtime — no TS codegen). No microfrontend talks to the
backend directly — `core` is the single auth/egress boundary. WASM MFEs call
`core`'s same-origin BFF over `fetch` (the backend's `tonic-web` layer also allows
direct gRPC-Web when latency demands it).

## Event sourcing + CQRS

Two consistency boundaries, never joined in one transaction:

- **TigerBeetle = data plane** — authoritative money (balances, transfers,
  allocations). Never bookkept a second time in Postgres.
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

## Run matrix

| `nix run .#`          | What                                                        | Port                          |
| --------------------- | ----------------------------------------------------------- | ----------------------------- |
| `dev`                 | postgres + tigerbeetle + redis + piggybank + core + landing | —                             |
| `piggybank`           | hub server: core gRPC + auth tasks (tonic-web)              | `:50051` core / `:50052` auth |
| `core`                | Next.js host shell + BFF                                    | `:3000`                       |
| `landing`             | Next.js marketing site                                      | `:3001`                       |
| `db` / `tb` / `redis` | local Postgres / TigerBeetle / Redis                        | `:5432` / `:3033` / `:6379`   |

See [`flake.nix`](../flake.nix) for the apps and dev shell, and per-area READMEs
(e.g. [`clients/core/README.md`](../clients/core/README.md)) for details.

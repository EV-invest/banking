## Layout

| Path | What | Stack | Details |
| ---- | ---- | ----- | ------- |
| [`piggybank/core/`](piggybank/core) | hub server — gRPC services + composition root | Rust · tonic · sqlx (Postgres) · TigerBeetle | [piggybank/](piggybank) |
| [`piggybank/auth/`](piggybank/auth) | auth service + shared verification flow | Rust · tonic · JWKS | — |
| [`contracts/`](contracts) | gRPC wire contracts (`proto/` → tonic stubs) | Rust · tonic-build · proto3 | — |
| [`domain/`](domain) | shared domain types (pure, wasm-safe) over `ev::architecture` | Rust | — |
| [`cabinet/`](cabinet) | host shell + BFF + microfrontend runtime | Next.js 16 · TS · Tailwind | [README](cabinet/README.md) |

`domain` is the shared source of truth for types — the hub and every service repo
depend on it, never on each other. `contracts` (vendoring `proto/`) is the single
gRPC dependency other repos import; the published `@evinvest/uikit` is the shared
design source of truth for the clients. There is **no HTTP on the hub** — browser
traffic reaches it through the `cabinet` BFF, which proxies HTTP↔gRPC.

## Run

Every app is a flake app. `nix run` resolves the repo root at runtime, so there's
no need to enter the dev shell first.

| Command | Brings up | Port |
| ------- | --------- | ---- |
| `nix run .#dev` | everything: Postgres + TigerBeetle + Redis + piggybank + cabinet | — |
| `nix run .#piggybank` | hub server: core gRPC + auth tasks (tonic-web) | `:50051` core · `:50052` auth |
| `nix run .#cabinet` | Next.js host shell + BFF | 3000 |
| `nix run .#db` | local Postgres (cluster under `.pg/`, trust auth) | 5432 |
| `nix run .#tb` | local TigerBeetle (data under `.tb/`, single replica) | 3033 |
| `nix run .#redis` | local Redis (central auth refresh-token store) | 6379 |

`.#dev` starts Postgres first and waits for it before launching the rest; one
Ctrl-C tears the whole stack down. Per-area build, test, and layout details live
in each folder's README — see the table above.

A dev shell with the full toolchain (Rust nightly + `wasm32`, Node, Postgres,
TigerBeetle, Redis, protobuf) is auto-activated by `.envrc` +
direnv, or via `nix develop`.

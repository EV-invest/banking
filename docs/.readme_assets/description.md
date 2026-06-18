`fund` is the central public hub of the `EV Investment` platform — the one process
that owns the money, the identities, and the contracts. A Cargo + Turborepo
monorepo: a Rust gRPC hub (`piggybank`) and Next.js clients over a shared,
wasm-safe `domain` crate. Every other service lives in its own repo and talks
**only to the hub, over gRPC** — never to another service.

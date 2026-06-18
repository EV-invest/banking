# cabinet — host shell + BFF

The root constructor for the bank's clients. Two jobs:

1. **Microfrontend composition.** Every microfrontend (React or Rust/WASM, inline
   widget or whole page) is a self-registering **custom element**. `cabinet` mounts
   them with [`<RemoteElement>`](./shared/mfe/RemoteElement.tsx), resolving each by
   logical name from the [registry](./mfe-registry.json) (served at
   `/api/mfe-registry`). Remotes deploy independently — change the registry, not
   `cabinet`. Light DOM only (Tailwind v4 tokens break in shadow DOM).
   - Inline widget: render `<RemoteElement>` anywhere in a page.
   - Whole page: the catch-all route `app/(mfe)/[service]/[[...slug]]` mounts a page MFE.

2. **BFF.** Server-side route handlers proxy browser HTTP to the hub's tonic gRPC
   backend ([shared/bff/grpc.ts](./shared/bff/grpc.ts)). The BFF reads
   `contracts/proto` at runtime with `@grpc/proto-loader` — no TS codegen; `tonic`
   + `tonic-build` generate the Rust side. Smoke path: `GET /api/health` →
   `HealthService.Check`.

See [`docs/ARCHITECTURE.md`](../../docs/ARCHITECTURE.md) for the full contract and
the React / Rust-WASM producer recipes.

## Dev

```
nix run .#cabinet      # this app on :3000 (needs the backend on :50051)
nix run .#dev       # full stack: postgres + tigerbeetle + backend + cabinet
```

# cabinet frontend — host shell

The Next.js host shell for the bank's cabinet. Two jobs:

1. **Microfrontend composition.** Every microfrontend (React or Rust/WASM, inline
   widget or whole page) is a self-registering **custom element**. The host mounts
   them with [`<RemoteElement>`](./shared/mfe/RemoteElement.tsx), resolving each by
   logical name from the [registry](./mfe-registry.json) (read here server-side for
   page routes, and served to the browser at `/api/mfe-registry` by the backend).
   Remotes deploy independently — change the registry, not the host. Light DOM only
   (Tailwind v4 tokens break in shadow DOM).
   - Inline widget: render `<RemoteElement>` anywhere in a page.
   - Whole page: the catch-all route `app/(mfe)/[service]/[[...slug]]` mounts a page MFE.

2. **BFF proxy.** The BFF itself is a separate Rust service
   ([`../backend`](../backend)). This app keeps calling same-origin `/api/*`;
   [`next.config.ts`](./next.config.ts) rewrites those to the backend
   (`CABINET_BACKEND_URL`), so the `__Host-`/HttpOnly session cookie + CSRF model
   stays same-origin. The browser never holds a token.

See [`docs/ARCHITECTURE.md`](../../../docs/ARCHITECTURE.md) for the full contract and
the React / Rust-WASM producer recipes.

## Observability

Wired through the shared `@evinvest/*` libraries; every integration no-ops until
its env var is set, so local dev needs no configuration.

- **Analytics** (`@evinvest/analytics`) — `PostHogProvider` in
  [`application/providers.tsx`](./application/providers.tsx); capture from client
  components with `useCapture()`. Reads `NEXT_PUBLIC_POSTHOG_KEY` / `_HOST`.
- **Error monitoring** (`@evinvest/error-monitoring`) — `ErrorMonitoringProvider`
  (browser) in providers; server/runtime init + request-error capture in
  [`instrumentation.ts`](./instrumentation.ts); build integration via `withSentry`
  in [`next.config.ts`](./next.config.ts). Reads `NEXT_PUBLIC_SENTRY_DSN` (browser)
  / `SENTRY_DSN` (server) and the `SENTRY_ORG`/`PROJECT`/`AUTH_TOKEN` build vars.
- **Experiments** (`@evinvest/experiments`) — the A/B registry lives in
  [`application/experiments.ts`](./application/experiments.ts); sticky variant
  assignment runs in [`proxy.ts`](./proxy.ts). Read a variant in a Server
  Component with `getVariant`, render with `ExperimentTracker` (bridge `onEvent`
  to `useCapture`). Empty until the first experiment is declared.

See [`.env.example`](./.env.example) for the full env surface.

## Dev

```
nix run .#cabinet           # this app (proxies /api/* → the cabinet backend)
nix run .#cabinet-backend   # the BFF (needs piggybank; ports: flake.nix `ports`)
nix run .#dev               # full stack: postgres + tigerbeetle + redis + signer + piggybank + cabinet-backend + cabinet
```

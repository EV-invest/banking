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

## Motion

Everything animated in the cabinet comes from `shared/ui/motion`. Import the
primitives from the slice root; do **not** import `motion/react` in a view. The
point of the slice is that curves, durations and travel distances are decided
once — in `shared/ui/motion/tokens.ts` — rather than per screen, exactly as
colour is decided once in the uikit's variables.

| Primitive | Use for |
|---|---|
| `Settled` | a skeleton handing over to the content it stood in for |
| `Panel` + `PanelPresence` | a popup/drawer/result card mounting and unmounting |
| `PanelSwap` | the content inside an already-open panel changing record |
| `Reveal` | a single block arriving on mount |
| `Stagger` + `StaggerItem` | a short list arriving in sequence |

Rules:

- **This is a money surface.** Motion exists to say *what changed* — never to
  decorate, and never in a way that delays a figure landing on screen. The
  cabinet's `DUR.base` is deliberately about half the landing's.
- **`opacity` and `transform` only**, always once, always collapsing to a plain
  fade under `prefers-reduced-motion` (every primitive handles this; hand-written
  motion must call `useReducedMotion()` itself).
- **`Settled` does not animate when no skeleton was shown.** Data already present
  on the first render cuts straight in — fading it would invent a delay the data
  never had. It decides by adjusting state *during render*, not in an effect: an
  effect would paint the content opaque once and only then start the fade, and
  that flash is worse than the cut it replaced.
- **A panel that swaps records is two motions, not one.** Key `Panel` on whether
  the panel is open — never on which record it shows, or every row click plays a
  full exit and enter — and put `PanelSwap` inside for the record change. The
  admin users drawer (`views/admin/users`) is the reference.
- **Alerts and result cards that replace one another share one
  `PanelPresence`.** A submit that fails should swap review → error in place
  rather than emptying the column and refilling it. See `views/wallet/withdraw`
  and `views/invest/deal-panels`.
- **`layoutId` is for a marker that moves**, like the rail's active pill and the
  mobile tab bar's rule: one shared node that motion tracks from the old position
  to the new one, instead of a per-item background that blinks between rows.
- Note the eslint guard on arbitrary Tailwind values applies here too — motion
  values live in props and tokens, not in class strings.

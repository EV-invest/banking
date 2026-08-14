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

## i18n

Five locales (`en` `ru` `vi` `fr` `de`) through `@evinvest/i18n`, under the same
translation policy as the public site: English is canonical, and a translation
whose English source has since changed is refused and falls back to English. Run
`npm run i18n:check` — it fails on drift, not on untranslated keys.

**The locale is a cookie here, not a URL segment.** That asymmetry with the
conductor is deliberate. The public site prefixes every non-default locale
(`/ru/team`) because it must serve `hreflang` alternates to a crawler and must not
move already-indexed URLs. The cabinet is entirely behind auth: no crawler, no
indexed URL, and prefixing would mean restructuring all 19 routes around a
`[locale]` segment to buy nothing.

So `proxy.ts` negotiates `Accept-Language` once and mints a sticky `ev_locale`
cookie, and never overwrites it — a reader who picks English on a Russian laptop
stays in English. `currentLocale()` (`shared/config/locale.ts`) reads it; the root
layout feeds `I18nProvider` so both the `(app)` and `(auth)` groups are covered.

Catalogues live in `messages/<locale>/common.json`. English is a plain map; every
other locale records the English each entry was translated from, which is what
makes drift detectable:

```jsonc
{ "nav.wallet": { "en": "Wallet", "t": "Кошелёк" } }
```

Only the navigation is translated so far. The banking vocabulary in it —
*Treasury*, *Valuation & redemptions* — wants a native review before any locale
is offered to a real investor.

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

## Data

Every browser→BFF read goes through the cache in `shared/lib/resource.ts`. A view
does **not** call an entity client from a `useEffect`; it declares what it reads
and the cache decides whether that costs a request.

```ts
const wallet = useResource(walletResource);          // no argument
const nav = useResource(fundNavResource, service);   // keyed per fund
```

Why it exists: every screen used to own its own `useEffect(() => fetchX().then(setX))`,
so leaving a page threw its answer away. Moving between two screens of the same
account re-fetched the same balance and preceded each arrival with a skeleton —
which reads as loading a *different* account. Nothing was wrong with any one of
those reads; the problem was that none of them was shared.

Why not Next's `fetch` extension: `next: { revalidate, tags }` is a **server**
extension, applied to fetches Next issues while rendering. This cabinet reads
nothing on the server — `/api/*` is a rewrite straight to the Rust BFF
(`next.config.ts`), there are no route handlers, and every read is a browser call
carrying the user's session cookie. So the cache is client-side, but keeps Next's
vocabulary — `revalidate` in seconds, `tags`, and a `revalidateTag()` — because
the semantics are the same.

Rules:

- **Declare each read once**, in `entities/<x>/model/<x>-resource.ts`, with the
  window it stays fresh for and the tags its mutations move. Views import from
  `model/`, never from `api/`.
- **A mutation names what it moved**, it does not refetch. `submitWithdrawal`
  names `wallet · withdrawals · operations`, so the balance on *every* open
  surface follows — no call site has to know which screens are mounted. A write
  whose response IS the new state (`saveProfile`, `postValuation`) publishes it
  rather than invalidating.
- **`isLoading` is the only state that earns a skeleton.** It is true only when
  there is nothing to show and nothing has failed. Cached data renders on the
  first frame and refreshes behind itself; see the `Settled` rule below, which
  this pairs with exactly.
- **A failed refresh never blanks a figure.** The stale value stays and the error
  is reported beside it. Only a read that has *never* succeeded should surface as
  an error state.
- **`persist: true` is for non-personal data only** — the fund catalog and NAVs.
  A balance, a position, a profile or an operation stays in memory, which dies
  with the page. Sign-out clears both halves.
- **Warm ahead of the click.** `application/prefetch.ts` maps each route to the
  reads it makes; the rail and the tab bar warm the target on pointer or keyboard
  intent, and the shell warms the shared reads at idle. That is what makes the
  *first* visit to a screen skeleton-free too.

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

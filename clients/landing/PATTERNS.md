# Frontend patterns

Conventions for this Next.js 16 (App Router) + FSD codebase. Keep them — they're
what makes the app fast (small client bundle, SSR/streaming) and the code easy to
reason about.

Reference: Next.js — [Server and Client Components](https://nextjs.org/docs/app/getting-started/server-and-client-components).

---

## 1. Server by default, `"use client"` at the leaf

**Server Components are the default. Add `"use client"` only to the smallest
piece that actually needs the client — never to a whole section "to be safe".**

A file needs `"use client"` only for:

- state / event handlers (`useState`, `onClick`, `onChange`)
- lifecycle (`useEffect`) or custom hooks
- browser-only APIs (`window`, `document`, `localStorage`)

Everything else stays a Server Component (zero JS shipped, server-rendered).

> `"use client"` marks a **boundary**: once a file has it, every module it imports
> and every component it _renders directly_ is pulled into the client bundle. So
> putting it high in the tree drags huge static subtrees client-side. Push it down.

### Server wraps client islands

The server is always the wrapper; client bits are **islands** inside it. Two ways
to keep server-rendered UI inside a client component (neither pulls the server
code into the client bundle):

- **`children` / props slot** — a Client Component renders `{children}`; the
  Server Component passes server-rendered elements in.
  Example: `ExperimentTracker` (client) wraps server `<TeamA/>` / `<TeamB/>`.
- **Extract the interactive leaf** — pull just the handler into a tiny client
  component and render it from the server parent.
  Examples: `hero/ui/hero-b-cta.tsx` (the only client bit of `HeroB`),
  `team/ui/team-placeholders.tsx` (the only client bit of `TeamA`/`TeamB`).

**Litmus test:** if a section is static except for one button, the section is a
Server Component and the button is the client island — not the other way around.

The exception is a component that is interactive _throughout_ (e.g. `HeroA`'s
scroll-zoom drives the whole visual from state) — that legitimately is one Client
Component. Even then, pass static sub-sections as `ReactNode` props from the
server wrapper so they stay out of the client bundle. Example: `HeroA` accepts
`stats={<HeroAStats />}` — the ribbon is SSR'd, the zoom shell is client-only.

### Context providers

React context needs a Client Component. Put `"use client"` on the provider, have
it accept `children`, and render it from a Server Component as deep as possible.
Server-Component children rendered _inside_ the provider's subtree still reach
client consumers nested below (see `TeamTracker` → `TeamPlaceholders`).

---

## 2. A/B testing — sections own it, pages don't

**A page must not know its sections are tested.** It renders `<Hero/>`,
`<Team/>` uniformly. Each section resolves its own variant.

Flow:

1. `proxy.ts` (root, Next 16 — nodejs runtime) assigns a sticky weighted cookie
   `ab_<key>` on first visit, per `shared/config/experiments.ts`. `Math.random`
   per-device, no user-id hashing.
2. A section's **Server** entry calls `await getVariant("<key>")`
   (`features/ab-variant`) — reads the cookie, returns a typed variant.
3. It picks the variant **element on the server**
   (`variant === "b" ? <X/> : <Y/>`), so only the served variant's chunk ships.
4. It wraps the chosen element in `<ExperimentTracker experiment="..." variant={v}>` —
   the thin client island that fires `${experiment}_exposed` on mount and provides
   variant context to all interactive islands in the subtree.

Add an experiment: add a key to `experiments`, build a server wrapper for the
section that branches on `getVariant` and wraps with `<ExperimentTracker>`, done.
The dev overlay (`DevAbPanel`) picks it up automatically.

> Reading the cookie makes the route dynamic — inherent to cookie-based A/B.

---

## 3. Analytics — one feature, event sink only

All instrumentation goes through `features/analytics` and `features/ab-variant`:

- `<PostHogProvider>` — mount once in the root layout.
- `<ExperimentTracker experiment="..." variant={v}>` — wraps a section's server
  wrapper; fires `${experiment}_exposed` on mount and provides context.
- `useExperimentEvent()` — inside any client island in the section, returns a
  `track(action, props?, handler?)` function that emits `${experiment}_${action}`:
  ```tsx
  // fire directly
  track("cta_clicked", { cta: "explore" })

  // fire + run a side effect; handler controls order
  track("cta_clicked", { cta: "explore" }, (fire) => {
    fire();
    doSomethingElse();
  })
  ```
- `capture(event, props)` — raw escape hatch for events unrelated to an experiment.

Rules:

- **Never import `posthog-js` outside `features/analytics`.** Swap/consent/init
  live in one place.
- Analytics **records**, it never **decides** what to render (bucketing is owned
  by `proxy.ts` + cookies).
- Call only from Client Components — never from Server Components / `next/headers`.
- Event names are `snake_case`, scoped `<surface>_<thing>_<action>`, and stable
  (they're the dashboard contract). Props are primitive and non-PII.

See `features/analytics/client.ts` for the full "how to add an event" guide.

---

## 4. Component size — 100–120 line cap

**A component file should not exceed 120 lines.** When it does, split it.

The cap forces the right decomposition: long components are almost always mixing
static layout with interactive logic, or cramming two distinct visual concerns
into one file. Splitting reveals the structure and usually also reduces the client
bundle (the static part becomes a Server Component).

Practical split strategies:

| Symptom | Split |
|---|---|
| Static block inside a client component | Extract as a Server Component, pass as `ReactNode` prop |
| Two independent visual regions | One file per region, compose in the parent |
| Reusable card / row layout | Move to `ui/` shared or a `*-shared.tsx` sibling |
| Complex hook logic | Extract to a `use*.ts` hook file |

> The 120-line budget includes imports and blank lines. JSDoc counts too.

---

## 5. FSD layering (where things go)

- `app/` — routing, root layout. Server by default.
- `views/` — page composition (e.g. `HomeView`). Agnostic of A/B.
- `features/` — behaviour slices: `analytics`, `ab-variant`, `investment-calculator`.
- `entities/` — domain data (`team`, …).
- `shared/` — reusable, app-agnostic: `ui/`, `lib/`, `config/`, `hooks/`.

Each slice exposes a public API via its `index.ts`; import from the slice root,
not deep paths.

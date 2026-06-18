# landing

Public marketing site for the EV Investment fund — Next.js 16 (App Router) +
React 19, laid out with Feature-Sliced Design, styled with Tailwind v4. Serves on
`:3000`.

## Layout

```
app/          routing + root layout (Server Components by default)
views/        page composition (HomeView), agnostic of A/B
features/     behaviour slices: analytics, ab-variant, investment-calculator
entities/     domain data (team, …)
shared/       app-agnostic ui/, lib/, config/, hooks/
application/  styles/globals.css → imports ../public/tokens.css
public/       static assets
tests/        per-section Playwright visual baselines
proxy.ts      Next 16 proxy — sticky weighted A/B cookie assignment
```

FSD import order `app → views → features → entities → shared`; import from a
slice's `index.ts`, never deep paths. Conventions live in [`PATTERNS.md`](./PATTERNS.md).

## Run only landing

```sh
nix run .#landing            # → http://localhost:3000
```
Or inside the dev shell (`.envrc` + direnv):
```sh
cd landing && npm install && npm run dev
```
Tailwind builds automatically inside Next via the `@tailwindcss/postcss` plugin —
no separate step. It imports the shared tokens from
[`../public/tokens.css`](../public/tokens.css) through `application/styles/globals.css`;
edit the tokens there and Next hot-reloads.

## Checks

```sh
npm run lint                 # eslint
npm run check                # tsc --noEmit
```

## Visual-regression tests

Per-section Playwright screenshots in [`tests/`](./tests) — one baseline per
addressable section (`#hero`, `#portfolio`, `#calculator`, `#research`, `#team`,
plus header/footer). Browsers come from nixpkgs, pinned to the `@playwright/test`
revision via the flake, so screenshots are reproducible.
```sh
npm run test:visual          # compare against committed baselines
npm run test:visual:update   # regenerate baselines after an intentional UI change
```
> Linux-only via nix; skip locally on macOS.

> The landing is a static marketing site — it does not call the hub backend, so
> there is no API client here.

## Design system (Figma)

The visual language is mirrored in a Figma file
([`Landing`](https://www.figma.com/design/e0V2P1cQpEFRuXTeNtEMh6/Landing)), kept
**code-first**: [`application/styles/globals.css`](./application/styles/globals.css)
plus the shared [`../public/tokens.css`](../public/tokens.css) are the source of
truth; the Figma side is conformed to them, never the reverse.

- **Tokens → Figma Variables.** Three collections mirror the CSS tokens 1:1, each
  with `var(--token)` code syntax so Dev Mode round-trips cleanly: `ev/color`
  (brand primitives `main-*` + neutrals), `ev/semantic` (shadcn roles aliased onto
  the primitives), `ev/radius` (the `--radius` scale).
- **Components.** The shadcn `bricks` are rebuilt as Figma variant-sets bound to
  those Variables (Button, Badge, Input/Field, Checkbox, Switch, Card, Select,
  Tabs, Accordion, Tooltip), on a dark navy surface. Fonts: Inter (sans) +
  Playfair (display).

> Code Connect (the design↔code mapping) is intentionally **not** wired up: it
> requires a Figma Organization/Enterprise plan and the file is on Pro. Tokens and
> variant-sets work regardless.

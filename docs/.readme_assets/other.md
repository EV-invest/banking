<!-- Per-area details live in each folder's README and the full architecture in docs/ARCHITECTURE.md — not duplicated here. -->

## Design

All UI lives in one Figma file (`e0V2P1cQpEFRuXTeNtEMh6`) — a dark-navy system in **Inter**, every value bound to `ev/color` · `ev/semantic` · `ev/radius` variables and shipped to clients as `@evinvest/uikit`.

| Surface | What | Figma |
| ------- | ---- | ----- |
| uikit | EV UIKit — tokens + component library (shadcn-class) | [node 10-2](https://www.figma.com/design/e0V2P1cQpEFRuXTeNtEMh6/Main?node-id=10-2) |
| landing | Public marketing site | [node 0-1](https://www.figma.com/design/e0V2P1cQpEFRuXTeNtEMh6/Main?node-id=0-1) |
| cabinet | Investor portal — `clients/core` host shell: **Fund** nav + **Products** (mounted service MFEs) + per-service surfaces; desktop + mobile | [node 75-3](https://www.figma.com/design/e0V2P1cQpEFRuXTeNtEMh6/Main?node-id=75-3) |
| admin | Operator console over the central hub (`piggybank`) + microservices — fleet health, users, MFE registry, feature flags; desktop + mobile | [node 346-27](https://www.figma.com/design/e0V2P1cQpEFRuXTeNtEMh6/Main?node-id=346-27) |

Admin surfaces **Sentry** (errors + tracing across hub and services) and **PostHog** (product analytics, feature flags, A/B experiments).

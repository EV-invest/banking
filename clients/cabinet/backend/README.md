# cabinet-backend

The cabinet's **BFF** (backend-for-frontend): a standalone, stateless HTTP service that is
the cabinet's single auth/egress boundary. It runs the OAuth confidential-client flow,
holds the user's session (and the issued token pair) server-side, and proxies the browser's
same-origin `/api/*` JSON requests to two gRPC planes:

- **concierge** (identity) — OAuth `Exchange`/`Refresh`/`Logout`, `ListSessions`/`RevokeSession`,
  and `UserDirectory` `GetMe`/`UpdateProfile`.
- **piggybank** (money) — `WalletService`, `FundsService`, `HealthService`.

The browser never sees a token: it holds only an opaque `ev_session` cookie (HttpOnly) and
a readable `ev_csrf` cookie (double-submit). The Next.js frontend
(`clients/cabinet/frontend`) reaches this service through a same-origin `/api/*` rewrite, so
cookies and CSRF behave exactly as before the BFF moved out of Next.

## Layout

| Module | Role |
| ------ | ---- |
| `config.rs` | env-sourced `Config` |
| `state.rs` | `AppState` + `Grpc` (lazy channels + typed client calls to both planes) |
| `session.rs` | in-process session store + single-flight token refresh |
| `oauth.rs` | PKCE/state/nonce, the Google authorize URL, the OAuth transaction store |
| `cookies.rs` | `__Host-`/HttpOnly/SameSite cookie identity (must match the frontend) |
| `dto.rs` | browser-facing JSON DTOs (snake_case; i64/u64 as strings, like the old proto-loader BFF) |
| `error.rs` | gRPC status → HTTP status + `{ "error": … }` body |
| `routes/` | one handler per endpoint: `auth`, `identity`, `money`, `system` |

## Run

`nix run .#cabinet-backend` (listens on `:4000`). It needs the piggybank hub on `:50051`
(`nix run .#piggybank`, or `.#dev`); identity flows additionally need the concierge runner
on `:50061`, started from the sibling `concierge` repo. Config defaults live in
`.env.example` (copy to `.env`); any value already in the environment wins.

> **Note:** end-to-end login depends on concierge's `AuthService`/`UserDirectory` being
> implemented (currently scaffold stubs) and on piggybank trusting concierge-issued access
> tokens for the money plane. The routing/wiring here is complete and forward-ready.

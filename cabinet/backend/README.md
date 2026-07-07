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
(`cabinet/frontend`) reaches this service through a same-origin `/api/*` rewrite, so
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

`nix run .#cabinet-backend`. It needs the piggybank hub (`nix run .#piggybank`, or
`.#dev`); identity flows additionally need the concierge runner, started from the
sibling `concierge` repo. Every port comes from the flake's `ports` attrset;
secrets live in `.env.example` (copy to `.env`).

> **Network segmentation.** `CABINET_BACKEND_BIND` stays loopback-reachable only
> because this process holds every user's tokens and its only request-auth is the session
> cookie. It must be reached **only** through the frontend's same-origin `/api/*` reverse
> proxy. Widen the bind (`0.0.0.0`) only behind an upstream firewall that keeps `/api/*` off
> any public interface — see [`docs/ARCHITECTURE.md`](../../../docs/ARCHITECTURE.md).

> **Two token pairs (cross-plane trust).** The BFF spans both planes, which sign tokens under
> separate issuers and distinct `aud` (concierge `aud=concierge`, banking `aud=banking-core`).
> The session holds a token pair **per plane**: the concierge pair authorizes identity RPCs, a
> separate banking pair authorizes money RPCs. The BFF forwards each plane its **own** token and
> never the other plane's — so a leaked identity token cannot move money. The banking pair is
> **exchange-based**: after the concierge sign-in, the BFF calls banking `AuthService.IssueUserToken`
> (the concierge→banking seam) — authenticated by the shared `BANKING_ISSUANCE_TOKEN`, NOT
> piggybank trusting concierge's issuer — and banking mints an `aud=banking-core` pair for the
> bridge-mirrored user. See [`docs/ARCHITECTURE.md`](../../../docs/ARCHITECTURE.md).
>
> **Note:** both planes are now implemented end-to-end. The money pair is minted at login
> (best-effort) and re-minted/rotated on demand; if the bridge hasn't mirrored a brand-new user
> yet, the money routes surface `NotConfigured` (503) until the next request re-mints. Cross-plane
> revocation: a concierge SUSPENDED freezes money ops immediately (per-op gate); a SESSIONS_REVOKED
> invalidates the money family within the banking access TTL (the revoke is enforced at refresh).

## Live Google login (e2e)

The full login chain — `/api/auth/login` → Google consent → `/api/auth/callback` →
concierge `Exchange` → user provisioned + first-party tokens minted → session opened — is
wired and contract-tested, but exercising it live needs real Google credentials. One-time
setup:

1. **Google Cloud Console → Credentials → OAuth client ID → Web application.** Add
   `http://localhost:$CABINET_FRONTEND_PORT/cabinet/api/auth/callback` as an Authorized redirect URI (it must match
   `AUTH_REDIRECT_URI` here *and* the `redirect_uri` concierge sends to Google). Note the
   client id + secret.
2. **concierge** (the secret lives only here) — in its `.env`: `GOOGLE_CLIENT_ID`,
   `GOOGLE_CLIENT_SECRET`, a signing key so it can mint/serve JWKS (`AUTH_SIGNING_KEY_PEM` +
   `AUTH_SIGNING_KID` + `AUTH_JWKS_JSON`; generate per its `.env.example`), and `DATABASE_URL`
   (the user is provisioned on first sign-in). Without a signing key or Google client,
   `Exchange` returns `NotConfigured`.
3. **this BFF** — in its `.env`: `GOOGLE_CLIENT_ID` (the same *public* id), unchanged
   `AUTH_REDIRECT_URI`, `AUTH_COOKIE_SECURE=false` (http://localhost rejects `__Host-`+Secure),
   plus a `BANKING_ISSUANCE_TOKEN` that
   **matches the banking core's** `BANKING_ISSUANCE_TOKEN` (the shared concierge→banking seam token).

Run, then sign in:

1. `nix run .#db` (the shared Postgres); start the concierge runner (from its repo)
   and the piggybank hub (`nix run .#piggybank`).
2. `nix run .#cabinet-backend` and the frontend (which rewrites `/api/*` here).
3. Open the frontend (`http://localhost:$CABINET_FRONTEND_PORT`), sign in, complete Google consent.
4. Verify: `GET /api/auth/session` returns `authenticated`; concierge's `users` table has the
   row and `user_outbox` has its `CREATED` event (the banking plane pulls it over the bridge);
   once the bridge mirrors the user, the money routes (e.g. `GET /api/wallet`) serve a real
   `aud=banking-core` token instead of `503`.

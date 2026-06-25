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

`nix run .#cabinet-backend` (listens on `127.0.0.1:4000`). It needs the piggybank hub on
`:50051` (`nix run .#piggybank`, or `.#dev`); identity flows additionally need the concierge
runner on `:50061`, started from the sibling `concierge` repo. Config defaults live in
`.env.example` (copy to `.env`); any value already in the environment wins.

> **Network segmentation.** `CABINET_BACKEND_BIND` defaults to loopback (`127.0.0.1:4000`)
> because this process holds every user's tokens and its only request-auth is the session
> cookie. It must be reached **only** through the frontend's same-origin `/api/*` reverse
> proxy. Widen the bind (`0.0.0.0`) only behind an upstream firewall that keeps `/api/*` off
> any public interface — see [`docs/ARCHITECTURE.md`](../../../docs/ARCHITECTURE.md).

> **Two token pairs (cross-plane trust).** The BFF spans both planes, which sign tokens under
> separate issuers and distinct `aud` (concierge `aud=concierge`, banking `aud=banking-core`).
> The session holds a token pair **per plane**: the concierge pair authorizes identity RPCs, a
> separate banking pair authorizes money RPCs. The BFF forwards each plane its **own** token and
> never the other plane's — so a leaked identity token cannot move money. The banking token is
> **exchange-based** (minted by the banking plane via a concierge→banking exchange / banking
> issuance route), NOT piggybank trusting concierge's issuer — see
> [`docs/ARCHITECTURE.md`](../../../docs/ARCHITECTURE.md).
>
> **Note:** concierge's `AuthService`/`UserDirectory` are now implemented (the identity plane
> mints real first-party tokens and provisions users), so identity login works end-to-end once
> Google credentials are configured — see *Live Google login* below. What remains is the
> concierge→banking token-exchange seam that mints the money-plane (`aud=banking-core`) token:
> until it exists no banking token is minted, so the money routes surface `NotConfigured` (503)
> rather than forwarding the identity token. The identity routes and the wiring here are complete.

## Live Google login (e2e)

The full login chain — `/api/auth/login` → Google consent → `/api/auth/callback` →
concierge `Exchange` → user provisioned + first-party tokens minted → session opened — is
wired and contract-tested, but exercising it live needs real Google credentials. One-time
setup:

1. **Google Cloud Console → Credentials → OAuth client ID → Web application.** Add
   `http://localhost:3000/api/auth/callback` as an Authorized redirect URI (it must match
   `AUTH_REDIRECT_URI` here *and* the `redirect_uri` concierge sends to Google). Note the
   client id + secret.
2. **concierge** (the secret lives only here) — in its `.env`: `GOOGLE_CLIENT_ID`,
   `GOOGLE_CLIENT_SECRET`, a signing key so it can mint/serve JWKS (`AUTH_SIGNING_KEY_PEM` +
   `AUTH_SIGNING_KID` + `AUTH_JWKS_JSON`; generate per its `.env.example`), and `DATABASE_URL`
   (the user is provisioned on first sign-in). Without a signing key or Google client,
   `Exchange` returns `NotConfigured`.
3. **this BFF** — in its `.env`: `GOOGLE_CLIENT_ID` (the same *public* id), unchanged
   `AUTH_REDIRECT_URI`, `AUTH_COOKIE_SECURE=false` (http://localhost rejects `__Host-`+Secure),
   and `CONCIERGE_GRPC_ADDR=http://127.0.0.1:50061`.

Run, then sign in:

1. `nix run .#db` (this repo) and concierge's Postgres; start the concierge runner (`:50061`).
2. `nix run .#cabinet-backend` (`:4000`) and the frontend (`:3000`, which rewrites `/api/*` here).
3. Open `http://localhost:3000`, sign in, complete Google consent.
4. Verify: `GET /api/auth/session` returns `authenticated`; concierge's `users` table has the
   row and `user_outbox` has its `CREATED` event (the banking plane pulls it over the bridge).

Money-plane routes stay `503` until the concierge→banking exchange seam lands (above) —
identity login itself is fully functional.

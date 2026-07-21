// The single point where this app reads its environment. Every other module
// imports from `config` instead of touching `process.env`.
//
// Backed by @evinvest/settings — the TypeScript mirror of the `settings` Cargo
// feature of ev_lib. Settings are created lazily on first access so tests that
// mutate `process.env` between calls still work; `assertConfig()` (called from
// `instrumentation.ts` at server boot) forces validation before the server
// accepts traffic. The client/server split is automatic: only NEXT_PUBLIC_*
// keys reach the browser bundle; accessing a server key there is a no-op.

import { createSettings, bool, optional as opt, str, url } from "@evinvest/settings";

// ── Lazy settings — re-created on every access ─────────────────────────
// No caching: tests mutate `process.env` between calls, and the overhead of
// validating 8 vars is negligible. `assertConfig()` forces a validation at
// server boot (CrashLoopBackOff on failure) before any traffic is accepted,
// matching the `@evinvest/settings` "fail at boot" contract.

function getSettings() {
  return createSettings({
    server: {
      // Where next.config.ts rewrites same-origin /api/* to (the Rust BFF).
      // Required — always set by the Nix build env and container contract.
      CABINET_BACKEND_URL: url(),
      // `__Host-`/Secure cookies need HTTPS; explicit override, else infer from NODE_ENV.
      AUTH_COOKIE_SECURE: opt(bool()),
      // Extra CSP/MFE allow-list origins not present in mfe-registry.json.
      MFE_ALLOWED_ORIGINS: opt(str()),
      // Next.js sets NODE_ENV during build and at runtime; unset in tests.
      NODE_ENV: opt(str()),
    },
    clientPrefix: "NEXT_PUBLIC_",
    client: {
      NEXT_PUBLIC_MFE_ALLOWED_ORIGINS: opt(str()),
      NEXT_PUBLIC_POSTHOG_HOST: opt(str()),
      NEXT_PUBLIC_SENTRY_DSN: opt(str()),
    },
    runtimeEnv: {
      CABINET_BACKEND_URL: process.env.CABINET_BACKEND_URL,
      AUTH_COOKIE_SECURE: process.env.AUTH_COOKIE_SECURE,
      MFE_ALLOWED_ORIGINS: process.env.MFE_ALLOWED_ORIGINS,
      NODE_ENV: process.env.NODE_ENV,
      NEXT_PUBLIC_MFE_ALLOWED_ORIGINS: process.env.NEXT_PUBLIC_MFE_ALLOWED_ORIGINS,
      NEXT_PUBLIC_POSTHOG_HOST: process.env.NEXT_PUBLIC_POSTHOG_HOST,
      NEXT_PUBLIC_SENTRY_DSN: process.env.NEXT_PUBLIC_SENTRY_DSN,
    },
  });
}

// ── Derived values (mirror the old config getters) ─────────────────────

function computeAuthCookieSecure(): boolean {
  const s = getSettings();
  return s.AUTH_COOKIE_SECURE ?? s.NODE_ENV === "production";
}

function computeIsProduction(): boolean {
  return getSettings().NODE_ENV === "production";
}

function computeIsDevelopment(): boolean {
  return getSettings().NODE_ENV === "development";
}

// ── Backward-compatible `config` export ────────────────────────────────
// The rest of the codebase imports `{ config }` from this module. Keep the
// shape exactly — lazy getters backed by @evinvest/settings validators.
//
// On the client, `getSettings()` returns only client keys; server getters
// short-circuit to undefined when `window` exists (matches the old behavior
// where `process.env[name]` returns undefined in the browser).

const _isServer = typeof window === "undefined";

export const config = ((): Readonly<{
  backendUrl: string;
  authCookieSecure: boolean;
  mfeAllowedOrigins: string | undefined;
  isProduction: boolean;
  isDevelopment: boolean;
  public: Readonly<{
    mfeAllowedOrigins: string | undefined;
    posthogHost: string | undefined;
    sentryDsn: string | undefined;
  }>;
}> => {
  return Object.freeze({
    get backendUrl(): string {
      return _isServer ? getSettings().CABINET_BACKEND_URL : (undefined as unknown as string);
    },
    get authCookieSecure(): boolean {
      return _isServer ? computeAuthCookieSecure() : false;
    },
    get mfeAllowedOrigins(): string | undefined {
      return _isServer ? getSettings().MFE_ALLOWED_ORIGINS : undefined;
    },
    get isProduction(): boolean {
      return _isServer ? computeIsProduction() : false;
    },
    get isDevelopment(): boolean {
      return _isServer ? computeIsDevelopment() : true;
    },
    public: Object.freeze({
      get mfeAllowedOrigins(): string | undefined {
        return getSettings().NEXT_PUBLIC_MFE_ALLOWED_ORIGINS;
      },
      get posthogHost(): string | undefined {
        return getSettings().NEXT_PUBLIC_POSTHOG_HOST;
      },
      get sentryDsn(): string | undefined {
        return getSettings().NEXT_PUBLIC_SENTRY_DSN;
      },
    }),
  });
})();

/**
 * Eagerly assert the config surface is valid. Called from `instrumentation.ts`
 * at server boot — fails the deploy (CrashLoopBackOff) before any traffic is
 * accepted. With lazy settings, this is the moment validation actually runs.
 *
 * During `next build` the instrumentation gate (`NEXT_RUNTIME === "nodejs"`)
 * prevents this from evaluating in the Edge bundle.
 */
export function assertConfig(): void {
  if (process.env.NEXT_PHASE === "phase-production-build") return;
  // Touching the getters triggers `getSettings()` → creates + validates.
  void config.backendUrl;
  void config.authCookieSecure;
  void config.mfeAllowedOrigins;
  void config.isProduction;
  void config.isDevelopment;
  void config.public.mfeAllowedOrigins;
  void config.public.posthogHost;
  void config.public.sentryDsn;
}

// Keep the old helpers for any remaining callers outside this module.
export function required(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`missing required env var ${name}`);
  return value;
}

export function optional(name: string): string | undefined {
  return process.env[name] || undefined;
}

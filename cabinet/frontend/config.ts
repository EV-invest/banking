// The single point where this app reads its environment. Every other module
// imports from `config` instead of touching `process.env`, so the app's env
// surface is one typed object (and an ESLint rule bans process.env elsewhere).
//
// Access is lazy: `next build` evaluates next.config.ts (and imports its module
// graph) with the deploy env absent, so eager reads/throws would break the build.
// Each field is a getter that reads on first use, so a required var throws —
// naming itself — only when actually consumed at runtime (see `required`).
//
// The client/server split is physical: NEXT_PUBLIC_* are the only vars Next
// inlines into browser bundles, so they live under `config.public` and are the
// only fields a Client Component may read. Everything else is server-only.

export function required(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`missing required env var ${name}`);
  return value;
}

function optional(name: string): string | undefined {
  return process.env[name] || undefined;
}

export const config = {
  // Where next.config.ts rewrites same-origin /api/* to (the Rust BFF). No
  // default: topology is owned by flake.nix, which always exports it.
  get backendUrl(): string {
    return required("CABINET_BACKEND_URL");
  },
  // `__Host-`/Secure cookies need HTTPS; explicit override, else infer from mode.
  get authCookieSecure(): boolean {
    const raw = optional("AUTH_COOKIE_SECURE");
    return raw !== undefined ? raw === "true" : config.isProduction;
  },
  // Extra CSP/MFE allow-list origins not present in mfe-registry.json.
  get mfeAllowedOrigins(): string | undefined {
    return optional("MFE_ALLOWED_ORIGINS");
  },
  get isProduction(): boolean {
    return process.env.NODE_ENV === "production";
  },
  get isDevelopment(): boolean {
    return process.env.NODE_ENV === "development";
  },
  // Browser-visible env (NEXT_PUBLIC_*): the only fields Client Components may
  // read, inlined at build time. Read server-side too (CSP construction).
  public: {
    get mfeAllowedOrigins(): string | undefined {
      return optional("NEXT_PUBLIC_MFE_ALLOWED_ORIGINS");
    },
    get posthogHost(): string | undefined {
      return optional("NEXT_PUBLIC_POSTHOG_HOST");
    },
    get sentryDsn(): string | undefined {
      return optional("NEXT_PUBLIC_SENTRY_DSN");
    },
  },
} as const;

// The identity plane's sign-in surface, reached at the site root (`/api/auth/*`, which the
// conductor sends to concierge) — not through the cabinet's BFF, which never sees a
// credential. Every answer is `{"ok":true,…}` or `{"error":"<code>"}` from one closed
// vocabulary; the dialog words the code.

import { csrfHeader } from "@/shared/lib/csrf-client";

export type AuthError =
  | "origin"
  | "captcha"
  | "captcha_unavailable"
  | "invalid_email"
  | "throttled"
  | "invalid_code"
  | "code_expired"
  | "attempts_exceeded"
  | "email_taken"
  | "weak_password"
  | "invalid_credentials"
  | "password_locked"
  | "disabled"
  | "unauthenticated"
  | "csrf"
  | "invalid_username"
  | "username_taken"
  | "internal";

export type AuthAnswer<T = Record<string, unknown>> = { ok: true; body: T } | { ok: false; error: AuthError };

async function post<T>(path: string, body: unknown, signedIn = false): Promise<AuthAnswer<T>> {
  let res: Response;
  try {
    res = await fetch(`/api/auth${path}`, {
      method: "POST",
      headers: { "content-type": "application/json", accept: "application/json", ...(signedIn ? csrfHeader() : {}) },
      body: JSON.stringify(body),
    });
  } catch {
    return { ok: false, error: "internal" };
  }
  const json = (await res.json().catch(() => null)) as (T & { error?: string }) | null;
  if (res.ok && json) return { ok: true, body: json };
  return { ok: false, error: (json?.error as AuthError | undefined) ?? "internal" };
}

export const requestCode = (email: string, turnstileToken: string) => post("/code/request", { email, turnstileToken });

export const verifyCode = (email: string, code: string) => post("/code/verify", { email, code });

export const passwordSignIn = (identifier: string, password: string, turnstileToken: string) =>
  post("/password/signin", { identifier, password, turnstileToken });

export const passwordSignUp = (email: string, password: string, verify: boolean, turnstileToken: string) =>
  post<{ verification: "sent" | "skipped" | "throttled" | "failed" }>("/password/signup", { email, password, verify, turnstileToken });

/** Mail the signed-in account a code for its own address. */
export const requestVerification = () => post("/email/verify/request", {}, true);

export const confirmVerification = (code: string) => post("/email/verify/confirm", { code }, true);

export const setPassword = (password: string, code: string) => post("/password/set", { password, code }, true);

export const setUsername = (username: string) => post<{ username: string }>("/username", { username }, true);

export interface SignInMethods {
  email: string;
  emailVerified: boolean;
  username: string | null;
  password: boolean;
  providers: string[];
}

export async function readMethods(): Promise<SignInMethods | null> {
  const res = await fetch("/api/auth/methods", { headers: { accept: "application/json" }, cache: "no-store" }).catch(() => null);
  return res?.ok ? ((await res.json()) as SignInMethods) : null;
}

/** The OAuth providers the plane signs in through, by their `?provider=` name. */
export const PROVIDERS = ["google", "github"] as const;
export type Provider = (typeof PROVIDERS)[number];

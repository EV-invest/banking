import { translator } from "@evinvest/i18n";

import { loginIntent, loginPageHref } from "@/features/auth/lib/login-intent";
import { loginHref } from "@/features/auth/lib/return-to";
import { messagesFor } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";
import { BrandPanel } from "@/views/login/ui/brand-panel";
import { LoginViewSignal } from "@/views/login/ui/login-view-signal";
import { SignInPanel } from "@/views/login/ui/sign-in-panel";

// The `?error=` values the shell's auth callback redirects with, mapped to catalogue keys.
const ERROR_KEYS: Record<string, string> = {
  denied: "auth.err.denied",
  invalid: "auth.err.invalid",
  exchange: "auth.err.exchange",
};

export type LoginSearchParams = { error?: string; returnTo?: string; intent?: string };

// The cabinet sign-in (Figma `cabinet/login`): a branded left panel + the sign-in panel.
// Auth is Google-only for now — one action both signs in and (on a first login) provisions
// the account at the hub, so there is no separate email/password or sign-up flow; a
// newcomer and a returning reader see different copy around the same button (#391).
//
// A true server component, so there is no `I18nProvider` above it and `useT()` is not
// available: the locale comes from the URL via `currentLocale()` and the catalogue is
// bound here, the same shape `views/status/ui/localised-status.tsx` uses.
export async function LoginView({ searchParams }: { searchParams: Promise<LoginSearchParams> }) {
  const { error, returnTo, intent: rawIntent } = await searchParams;
  const locale = await currentLocale();
  const t = translator(messagesFor(locale), locale);
  const message = error ? t(ERROR_KEYS[error] ?? "auth.err.generic") : null;
  const intent = loginIntent(rawIntent);
  // `returnTo` arrives zone-relative (`/wallet`) and leaves for the shell as a site-root
  // page (`/{locale}/cabinet/wallet`) — the prefix goes on here and nowhere else, see
  // `features/auth/lib/return-to.ts`. The intent stays on this page: the shell never
  // sees it, and the OAuth round-trip lands on `returnTo` whichever state it began in.
  const href = loginHref(locale, returnTo);
  const switchHref = loginPageHref(intent === "signup" ? "login" : "signup", returnTo);

  return (
    <div className="flex min-h-[calc(100dvh-var(--ev-shell-offset,0px))]">
      <LoginViewSignal intent={intent} hasReturnTo={returnTo !== undefined} hasError={message !== null} />
      <BrandPanel t={t} locale={locale} />
      <SignInPanel intent={intent} href={href} switchHref={switchHref} message={message} t={t} />
    </div>
  );
}

import type { Translate } from "@evinvest/i18n";

import type { LoginIntent } from "@/features/auth/lib/login-intent";
import { Link } from "@/shared/ui/cabinet-link";
import { GoogleMark } from "@/views/login/ui/google-mark";
import { NextSteps } from "@/views/login/ui/next-steps";

// The sign-in panel, in one of two states (#391). A newcomer sent by a landing CTA
// (`?intent=signup`) is told what they are opening and what follows; a returning reader
// is welcomed back. The button is the same for both — one Google sign-in both signs in
// and, on a first login, provisions the account at the hub — so the states differ in
// the copy around it and in the one-line switch under it, never in the action.
export function SignInPanel({
  intent,
  href,
  switchHref,
  message,
  t,
}: {
  intent: LoginIntent;
  /** The shell's OAuth entry, with `returnTo` already prefixed. */
  href: string;
  /** This page in the other state, `returnTo` carried along. */
  switchHref: `/${string}`;
  message: string | null;
  t: Translate;
}) {
  const signup = intent === "signup";
  return (
    <div className="flex flex-1 items-center justify-center px-6 py-16">
      <div className="flex w-full max-w-100 flex-col gap-6">
        <div className="flex flex-col gap-2">
          <h1 className="text-3xl font-semibold text-ink">{signup ? t("auth.signup.title") : t("auth.welcomeBack")}</h1>
          <p className="text-sm text-ink-soft">{signup ? t("auth.signup.sub") : t("auth.signInSub")}</p>
        </div>

        {message && <p className="rounded-md border border-accent-error/40 bg-accent-error/10 px-3 py-2 text-sm text-accent-error">{message}</p>}

        {signup && <NextSteps t={t} />}

        <a
          href={href}
          className="flex h-10 w-full items-center justify-center gap-3 rounded-md bg-brand px-6 text-sm font-medium text-ink outline-none ring-1 ring-inset ring-white/10 transition-colors hover:bg-brand/80 focus-visible:ring-2 focus-visible:ring-ring"
        >
          <GoogleMark /> {t("auth.continueWithGoogle")}
        </a>

        <p className="text-center text-sm text-ink-soft">
          {signup ? t("auth.switch.haveAccount") : t("auth.switch.newHere")}{" "}
          <Link
            href={switchHref}
            className="rounded-sm font-medium text-primary-ink underline-offset-4 outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring"
          >
            {signup ? t("auth.switch.signIn") : t("auth.switch.openAccount")}
          </Link>
        </p>
      </div>
    </div>
  );
}

"use client";

import { useT } from "@evinvest/i18n/react";

import { type FormEvent, type ReactNode, useEffect, useState, useSyncExternalStore } from "react";

import {
  Button,
  Checkbox,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  Input,
  InputOTP,
  InputOTPGroup,
  InputOTPSlot,
  Label,
  Separator,
  Spinner,
} from "@evinvest/uikit";

import { type AuthError, PROVIDERS, type Provider, confirmVerification, passkeySignIn, passwordSignIn, passwordSignUp, requestCode, verifyCode } from "@/features/auth/api/auth-client";
import { passkeysSupported } from "@/features/auth/lib/webauthn";
import { loginHref } from "@/features/auth/lib/return-to";
import { useSignInDialog } from "@/features/auth/model/use-sign-in-dialog";
import { GithubMark, GoogleMark } from "@/features/auth/ui/provider-marks";
import { Turnstile } from "@/features/auth/ui/turnstile";
import { useLocale } from "@/shared/lib/cabinet-route";

type Screen = { kind: "code" } | { kind: "codeSent"; email: string } | { kind: "password" } | { kind: "signup" } | { kind: "verifyNow"; email: string };

const CODE_LENGTH = 6;
const RESEND_SECS = 60;

type T = ReturnType<typeof useT>;

function errorText(t: T, error: AuthError | "captcha_load"): string {
  switch (error) {
    case "invalid_code":
      return t("auth.err.invalidCode", "That code is not right. Try again.");
    case "attempts_exceeded":
      return t("auth.err.attemptsExceeded", "Too many tries. Send yourself a new code.");
    case "code_expired":
      return t("auth.err.codeExpired", "That code has expired. Send yourself a new code.");
    case "throttled":
      return t("auth.err.throttled", "Too many codes went to this address. Try again in a few minutes.");
    case "captcha":
      return t("auth.err.captcha", "Captcha verification failed. Please try again.");
    case "captcha_unavailable":
    case "captcha_load":
      return t("auth.err.captchaLoad", "Captcha failed to load. Please refresh the page.");
    case "invalid_email":
      return t("auth.err.invalidEmail", "Enter a valid email address.");
    case "email_taken":
      return t("auth.err.emailTaken", "An account already exists for that address. Sign in instead — with a code if you forgot the password.");
    case "weak_password":
      return t("auth.err.weakPassword", "Use at least 8 characters.");
    case "invalid_credentials":
      return t("auth.err.invalidCredentials", "Wrong email, username or password.");
    case "password_locked":
      return t("auth.err.passwordLocked", "Too many wrong passwords. Sign in with a code instead.");
    case "disabled":
      return t("auth.err.disabled", "This account is suspended.");
    case "passkey_cancelled":
      return t("auth.err.passkeyCancelled", "Passkey sign in was cancelled.");
    case "passkey_rejected":
      return t("auth.err.passkeyRejected", "That passkey is not registered here. Sign in with a code instead.");
    case "passkey_expired":
      return t("auth.err.passkeyExpired", "That took too long. Try the passkey again.");
    default:
      return t("auth.err.generic", "Sign-in failed. Please try again.");
  }
}

/** What an OAuth round trip that failed puts on the page it returns to (`?auth_error=`). */
function oauthErrorText(t: T, reason: string): string {
  switch (reason) {
    case "denied":
      return t("auth.err.denied", "Sign-in was cancelled.");
    case "invalid":
      return t("auth.err.invalid", "That sign-in attempt expired. Please try again.");
    case "exchange":
      return t("auth.err.exchange", "We couldn't complete sign-in. Please try again.");
    case "disabled":
      return errorText(t, "disabled");
    default:
      return errorText(t, "internal");
  }
}

/**
 * "Sign in or create an account" — one dialog for every method. Opened by `?login` on any
 * cabinet page (`useSignInDialog`), so a link, an AuthWall and the rail open the same one.
 * A success reloads the page it was opened over, now as the signed-in account's.
 */
export function SignInDialog() {
  const t = useT();
  const locale = useLocale();
  const dialog = useSignInDialog();
  const [screen, setScreen] = useState<Screen>({ kind: "code" });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [token, setToken] = useState<string | null>(null);
  const [tokenGeneration, setTokenGeneration] = useState(0);
  const [captchaBroken, setCaptchaBroken] = useState(false);
  // The dialog renders through a portal the server does not draw: opening it before
  // hydration would make the two disagree.
  const hydrated = useSyncExternalStore(
    () => () => {},
    () => true,
    () => false,
  );

  function go(next: Screen) {
    setError(null);
    setScreen(next);
  }

  /** Spend the challenge token on one request; the widget re-arms for the next. */
  function spend(): string {
    const spent = token ?? "";
    setTokenGeneration((g) => g + 1);
    return spent;
  }

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await action();
    } finally {
      setBusy(false);
    }
  }

  function close(open: boolean) {
    if (open || busy) return;
    go({ kind: "code" });
    dialog.hide();
  }

  const needsChallenge = screen.kind === "code" || screen.kind === "codeSent" || screen.kind === "password" || screen.kind === "signup";
  const human = token !== null && !captchaBroken;

  return (
    <Dialog open={hydrated && dialog.open} onOpenChange={close}>
      <DialogContent>
        {screen.kind === "code" && (
          <CodeScreen
            busy={busy}
            human={human}
            onSubmit={(email) =>
              run(async () => {
                const answer = await requestCode(email, spend());
                if (answer.ok) go({ kind: "codeSent", email });
                else setError(errorText(t, answer.error));
              })
            }
            onPassword={() => go({ kind: "password" })}
          >
            <Providers
              hrefFor={(provider) => loginHref(locale, dialog.returnTo, provider)}
              onPasskey={() =>
                run(async () => {
                  const answer = await passkeySignIn();
                  if (answer.ok) dialog.reload();
                  else setError(errorText(t, answer.error));
                })
              }
            />
          </CodeScreen>
        )}
        {screen.kind === "codeSent" && (
          <CodeEntry
            title={t("auth.code.title", "Check your email")}
            description={t("auth.code.desc", "We sent a 6 digit sign in code to {email}", { email: screen.email })}
            busy={busy}
            human={human}
            onCode={(code) =>
              run(async () => {
                const answer = await verifyCode(screen.email, code);
                if (answer.ok) dialog.reload();
                else setError(errorText(t, answer.error));
              })
            }
            onResend={() =>
              run(async () => {
                const answer = await requestCode(screen.email, spend());
                if (!answer.ok) setError(errorText(t, answer.error));
              })
            }
            onBack={() => go({ kind: "code" })}
          />
        )}
        {screen.kind === "password" && (
          <PasswordScreen
            busy={busy}
            human={human}
            onSubmit={(identifier, password) =>
              run(async () => {
                const answer = await passwordSignIn(identifier, password, spend());
                if (answer.ok) dialog.reload();
                else setError(errorText(t, answer.error));
              })
            }
            onCode={() => go({ kind: "code" })}
            onSignUp={() => go({ kind: "signup" })}
          />
        )}
        {screen.kind === "signup" && (
          <SignUpScreen
            busy={busy}
            human={human}
            onSubmit={(email, password, verify) =>
              run(async () => {
                const answer = await passwordSignUp(email, password, verify, spend());
                if (!answer.ok) return setError(errorText(t, answer.error));
                if (answer.body.verification === "sent") go({ kind: "verifyNow", email });
                else dialog.reload();
              })
            }
            onSignIn={() => go({ kind: "password" })}
          />
        )}
        {screen.kind === "verifyNow" && (
          <CodeEntry
            title={t("auth.verify.title", "Confirm your email")}
            description={t("auth.verify.desc", "Your account is ready. Enter the 6 digit code we sent to {email} to verify it, or skip and verify later in Settings.", { email: screen.email })}
            busy={busy}
            human
            onCode={(code) =>
              run(async () => {
                const answer = await confirmVerification(code);
                if (answer.ok) dialog.reload();
                else setError(errorText(t, answer.error));
              })
            }
            onBack={() => dialog.reload()}
            backLabel={t("auth.verify.later", "Verify later")}
          />
        )}
        {needsChallenge && (
          <Turnstile
            generation={tokenGeneration}
            onToken={setToken}
            onUnavailable={() => {
              setCaptchaBroken(true);
              setError(errorText(t, "captcha_load"));
            }}
          />
        )}
        <p role="status" className="min-h-4 text-xs leading-snug text-accent-error">
          {error ?? (screen.kind === "code" && dialog.oauthError ? oauthErrorText(t, dialog.oauthError) : null)}
        </p>
      </DialogContent>
    </Dialog>
  );
}

function Heading({ title, description }: { title: string; description?: string }) {
  return (
    <DialogHeader>
      <DialogTitle>{title}</DialogTitle>
      {description && <DialogDescription>{description}</DialogDescription>}
    </DialogHeader>
  );
}

function Switch({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button type="button" onClick={onClick} className="text-sm font-medium text-primary-ink underline-offset-4 hover:underline">
      {children}
    </button>
  );
}

function looksLikeEmail(raw: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(raw.trim());
}

function submitWith(handler: () => void) {
  return (event: FormEvent) => {
    event.preventDefault();
    handler();
  };
}

function CodeScreen({ busy, human, onSubmit, onPassword, children }: { busy: boolean; human: boolean; onSubmit: (email: string) => void; onPassword: () => void; children: ReactNode }) {
  const t = useT();
  const [email, setEmail] = useState("");
  return (
    <>
      <Heading title={t("auth.dialog.heading", "Sign in or create an account")} />
      <form className="flex flex-col gap-3" onSubmit={submitWith(() => onSubmit(email.trim()))}>
        <Label htmlFor="sign-in-email">{t("auth.field.email", "Email")}</Label>
        <Input id="sign-in-email" type="email" autoComplete="email" value={email} onChange={(e) => setEmail(e.target.value)} />
        <Button type="submit" disabled={busy || !human || !looksLikeEmail(email)}>
          {busy ? <Spinner /> : t("auth.useLoginCode", "Email me a code")}
        </Button>
      </form>
      <Switch onClick={onPassword}>{t("auth.usePasswordInstead", "Sign in with a password instead")}</Switch>
      {children}
    </>
  );
}

function Providers({ hrefFor, onPasskey }: { hrefFor: (provider: Provider) => string; onPasskey: () => void }) {
  const t = useT();
  const label: Record<Provider, string> = { google: "Google", github: "GitHub" };
  const mark: Record<Provider, ReactNode> = { google: <GoogleMark />, github: <GithubMark /> };
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-3 text-xs text-ink-soft">
        <Separator className="flex-1" />
        {t("auth.orContinueWith", "Or continue with")}
        <Separator className="flex-1" />
      </div>
      {passkeysSupported() && (
        <Button type="button" variant="outline" onClick={onPasskey}>
          {t("auth.usePasskey", "Use a passkey")}
        </Button>
      )}
      <div className="grid grid-cols-2 gap-2">
        {PROVIDERS.map((provider) => (
          // A full navigation: the provider's consent screen is another origin.
          <a key={provider} href={hrefFor(provider)} className="inline-flex h-10 items-center justify-center gap-2 rounded-md border border-border text-sm font-medium hover:bg-ink/5">
            {mark[provider]}
            {label[provider]}
          </a>
        ))}
      </div>
    </div>
  );
}

function CodeEntry({
  title,
  description,
  busy,
  human,
  onCode,
  onResend,
  onBack,
  backLabel,
}: {
  title: string;
  description: string;
  busy: boolean;
  human: boolean;
  onCode: (code: string) => void;
  onResend?: () => void;
  onBack: () => void;
  backLabel?: string;
}) {
  const t = useT();
  const [code, setCode] = useState("");
  const [wait, setWait] = useState(RESEND_SECS);

  useEffect(() => {
    if (wait <= 0) return;
    const timer = setTimeout(() => setWait((s) => s - 1), 1000);
    return () => clearTimeout(timer);
  }, [wait]);

  function change(raw: string) {
    const digits = raw.replace(/\D/g, "").slice(0, CODE_LENGTH);
    setCode(digits);
    if (digits.length === CODE_LENGTH && !busy) onCode(digits);
  }

  return (
    <>
      <Heading title={title} description={description} />
      <InputOTP maxLength={CODE_LENGTH} value={code} onChange={change} disabled={busy} aria-label={t("auth.code.label", "Code")} autoFocus>
        <InputOTPGroup>
          {Array.from({ length: CODE_LENGTH }, (_, i) => (
            <InputOTPSlot key={i} index={i} />
          ))}
        </InputOTPGroup>
      </InputOTP>
      <p className="text-xs text-ink-soft">{t("auth.code.checkSpam", "It can take a minute. Check your spam folder too.")}</p>
      <div className="flex items-center justify-between gap-3">
        <Switch onClick={onBack}>{backLabel ?? t("auth.code.back", "Use a different email")}</Switch>
        {onResend &&
          (wait > 0 ? (
            <span className="text-sm text-ink-soft">{t("auth.code.resendIn", "Send a new code in {seconds}s", { seconds: wait })}</span>
          ) : (
            <Switch
              onClick={() => {
                if (!human || busy) return;
                setCode("");
                setWait(RESEND_SECS);
                onResend();
              }}
            >
              {t("auth.code.resend", "Send a new code")}
            </Switch>
          ))}
      </div>
    </>
  );
}

function PasswordScreen({
  busy,
  human,
  onSubmit,
  onCode,
  onSignUp,
}: {
  busy: boolean;
  human: boolean;
  onSubmit: (identifier: string, password: string) => void;
  onCode: () => void;
  onSignUp: () => void;
}) {
  const t = useT();
  const [identifier, setIdentifier] = useState("");
  const [password, setPassword] = useState("");
  return (
    <>
      <Heading title={t("auth.dialog.heading", "Sign in or create an account")} />
      <form className="flex flex-col gap-3" onSubmit={submitWith(() => onSubmit(identifier.trim(), password))}>
        <Label htmlFor="sign-in-identifier">{t("auth.field.identifier", "Email or username")}</Label>
        <Input id="sign-in-identifier" autoComplete="username" value={identifier} onChange={(e) => setIdentifier(e.target.value)} />
        <Label htmlFor="sign-in-password">{t("auth.field.password", "Password")}</Label>
        <Input id="sign-in-password" type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} />
        <Button type="submit" disabled={busy || !human || !identifier.trim() || !password}>
          {busy ? <Spinner /> : t("auth.signIn", "Sign in")}
        </Button>
      </form>
      <div className="flex items-center justify-between gap-3">
        <Switch onClick={onCode}>{t("auth.useCodeInstead", "Sign in with a code instead")}</Switch>
        <Switch onClick={onSignUp}>{t("auth.createAccount", "Create an account")}</Switch>
      </div>
    </>
  );
}

function SignUpScreen({ busy, human, onSubmit, onSignIn }: { busy: boolean; human: boolean; onSubmit: (email: string, password: string, verify: boolean) => void; onSignIn: () => void }) {
  const t = useT();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [verify, setVerify] = useState(true);
  return (
    <>
      <Heading title={t("auth.signup.heading", "Create an account")} description={t("auth.signup.desc", "Your account works right away. A KYC check later needs a verified email.")} />
      <form className="flex flex-col gap-3" onSubmit={submitWith(() => onSubmit(email.trim(), password, verify))}>
        <Label htmlFor="sign-up-email">{t("auth.field.email", "Email")}</Label>
        <Input id="sign-up-email" type="email" autoComplete="email" value={email} onChange={(e) => setEmail(e.target.value)} />
        <Label htmlFor="sign-up-password">{t("auth.field.password", "Password")}</Label>
        <Input id="sign-up-password" type="password" autoComplete="new-password" value={password} onChange={(e) => setPassword(e.target.value)} />
        <label className="flex items-center gap-2 text-sm">
          <Checkbox checked={verify} onCheckedChange={setVerify} />
          {t("auth.signup.verifyNow", "Verify my email now")}
        </label>
        <Button type="submit" disabled={busy || !human || !looksLikeEmail(email) || password.length < 8}>
          {busy ? <Spinner /> : t("auth.signup.create", "Create account")}
        </Button>
      </form>
      <Switch onClick={onSignIn}>{t("auth.signup.haveAccount", "Already have an account? Sign in")}</Switch>
    </>
  );
}

import { Logo } from "@/application/layout/logo";
import { safeReturnTo } from "@/features/auth/lib/oauth";

const ERRORS: Record<string, string> = {
  denied: "Sign-in was cancelled.",
  invalid: "That sign-in attempt expired. Please try again.",
  exchange: "We couldn't complete sign-in. Please try again.",
};

// The cabinet sign-in (Figma `cabinet/login`): a branded left panel + the sign-in panel.
// Auth is Google-only for now — one action both signs in and (on a first login) provisions
// the account at the hub, so there is no separate email/password or sign-up flow yet.
export async function LoginView({ searchParams }: { searchParams: Promise<{ error?: string; returnTo?: string }> }) {
  const { error, returnTo } = await searchParams;
  const message = error ? (ERRORS[error] ?? "Sign-in failed. Please try again.") : null;
  const dest = safeReturnTo(returnTo ?? null);
  // Full navigation (not a client route): the BFF redirects to Google.
  const href = dest === "/" ? "/api/auth/login" : `/api/auth/login?returnTo=${encodeURIComponent(dest)}`;

  return (
    <div className="flex min-h-screen">
      {/* brand panel */}
      <aside className="relative hidden w-[600px] shrink-0 flex-col justify-between overflow-hidden bg-main-brand p-16 lg:flex">
        {/* big soft teal wash */}
        <div className="pointer-events-none absolute bottom-[-160px] left-24 size-[820px] rounded-full bg-[radial-gradient(circle,rgba(72,216,196,0.6),rgba(42,157,143,0.32)_46%,transparent_74%)] blur-3xl" />
        {/* brighter inner core */}
        <div className="pointer-events-none absolute bottom-20 left-64 size-[420px] rounded-full bg-[radial-gradient(circle,rgba(120,240,216,0.55),transparent_60%)] blur-2xl" />

        <div className="relative">
          <Logo className="h-10 w-auto text-main-mist" />
        </div>

        <div className="relative flex max-w-md flex-col gap-5">
          <p className="text-[13px] font-semibold tracking-[0.12em] text-main-accent-t1">EV INVEST</p>
          <h2 className="font-sans text-[40px] font-semibold leading-[46px] text-white">Private capital for Vietnam&apos;s coastal economy.</h2>
          <p className="text-base leading-6 text-main-mist/40">Institutional-grade access to vetted coastal real-estate and infrastructure funds — managed end to end.</p>
        </div>

        <div className="relative flex gap-8">
          <BrandStat value="18.4%" label="Avg. target IRR" />
          <BrandStat value="$120M+" label="Assets under mgmt" />
        </div>
      </aside>

      {/* sign-in panel */}
      <div className="flex flex-1 items-center justify-center px-6 py-16">
        <div className="flex w-full max-w-[400px] flex-col gap-6">
          <div className="flex flex-col gap-2">
            <h1 className="font-sans text-3xl font-semibold text-white">Welcome back</h1>
            <p className="text-[15px] leading-[22px] text-main-mist/40">Sign in to manage your investments.</p>
          </div>

          {message && <p className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">{message}</p>}

          <a
            href={href}
            className="flex h-10 w-full items-center justify-center gap-3 rounded-md bg-main-brand px-6 text-sm font-medium text-main-mist ring-1 ring-inset ring-white/10 transition-colors hover:bg-main-brand/80"
          >
            <GoogleMark /> Continue with Google
          </a>

          <p className="text-center text-sm text-main-mist/40">New to EV Invest? Continuing with Google sets up your cabinet.</p>
        </div>
      </div>
    </div>
  );
}

function BrandStat({ value, label }: { value: string; label: string }) {
  return (
    <div className="flex flex-col gap-1">
      <p className="text-[22px] font-semibold text-main-accent-t3">{value}</p>
      <p className="text-[13px] text-main-mist/40">{label}</p>
    </div>
  );
}

function GoogleMark() {
  return (
    <svg viewBox="0 0 24 24" className="size-[18px]" aria-hidden="true">
      <path fill="#4285F4" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92a5.06 5.06 0 0 1-2.2 3.32v2.76h3.56c2.08-1.92 3.28-4.74 3.28-8.09z" />
      <path fill="#34A853" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.56-2.76c-.98.66-2.23 1.06-3.72 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84A11 11 0 0 0 12 23z" />
      <path fill="#FBBC05" d="M5.84 14.11A6.6 6.6 0 0 1 5.49 12c0-.73.13-1.45.35-2.11V7.05H2.18A11 11 0 0 0 1 12c0 1.77.42 3.45 1.18 4.95l3.66-2.84z" />
      <path fill="#EA4335" d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.05l3.66 2.84C6.71 7.31 9.14 5.38 12 5.38z" />
    </svg>
  );
}

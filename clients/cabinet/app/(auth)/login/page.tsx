import { Boxes } from "lucide-react";

import { safeReturnTo } from "@/shared/auth/oauth";

const ERRORS: Record<string, string> = {
  denied: "Sign-in was cancelled.",
  invalid: "That sign-in attempt expired. Please try again.",
  exchange: "We couldn't complete sign-in. Please try again.",
};

export default async function LoginPage({ searchParams }: { searchParams: Promise<{ error?: string; returnTo?: string }> }) {
  const { error, returnTo } = await searchParams;
  const message = error ? (ERRORS[error] ?? "Sign-in failed. Please try again.") : null;
  const dest = safeReturnTo(returnTo ?? null);
  const href = dest === "/" ? "/api/auth/login" : `/api/auth/login?returnTo=${encodeURIComponent(dest)}`;

  return (
    <div className="w-full max-w-sm space-y-6 rounded-xl border border-border bg-card p-8">
      <div className="flex items-center gap-2 font-serif text-lg">
        <Boxes className="size-5 text-main-accent-t1" />
        <span>EV Banking</span>
      </div>

      <div className="space-y-1">
        <h1 className="text-2xl">Sign in</h1>
        <p className="text-sm text-muted-foreground">Access your investor cabinet.</p>
      </div>

      {message && <p className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">{message}</p>}

      {/* Full navigation (not a client route): the BFF redirects to Google. */}
      <a href={href} className="flex w-full items-center justify-center rounded-md bg-main-accent-t1 px-4 py-2.5 text-sm font-medium text-main-black transition hover:opacity-90">
        Continue with Google
      </a>
    </div>
  );
}

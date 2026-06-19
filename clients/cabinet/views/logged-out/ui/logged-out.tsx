import { Boxes } from "lucide-react";
import Link from "next/link";

export function LoggedOutView() {
  return (
    <div className="w-full max-w-sm space-y-6 rounded-xl border border-border bg-card p-8 text-center">
      <div className="flex items-center justify-center gap-2 font-serif text-lg">
        <Boxes className="size-5 text-main-accent-t1" />
        <span>EV Banking</span>
      </div>

      <div className="space-y-1">
        <h1 className="text-2xl">Signed out</h1>
        <p className="text-sm text-muted-foreground">You&apos;ve been signed out of your cabinet.</p>
      </div>

      <Link href="/login" className="flex w-full items-center justify-center rounded-md bg-main-accent-t1 px-4 py-2.5 text-sm font-medium text-main-black transition hover:opacity-90">
        Sign in again
      </Link>
    </div>
  );
}

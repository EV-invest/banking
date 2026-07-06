import type { ReactNode } from "react";

import { Header } from "@evinvest/uikit";

import { CONDUCTOR_NAV } from "@/application/layout/conductor-nav";
import { Sidebar } from "@/application/layout/sidebar";
import { SystemBanner } from "@/application/layout/system-banner";
import { AccountChip } from "@/features/account-chip";

// The authenticated app shell: the shared EV brand header (one shell across all
// surfaces — nav links lead back to the conductor's pages as hard cross-zone
// links; plain <a> is the uikit default) over a persistent left rail beside a
// scrollable content column. No footer here by design. Auth is enforced upstream
// in `proxy.ts` — unauthenticated requests are redirected to /login before this
// layout renders. The system banner (maintenance · read-only · announcement)
// mounts once here; (auth) pages have no session, so they are intentionally excluded.
export default function AppLayout({ children }: { children: ReactNode }) {
  return (
    <>
      {/* The compact variant is a fixed, opaque 4rem bar — app content sits directly
          beneath it (no hero), so the marketing bar's transparent scroll state would
          bleed. Its fixed height lets the sidebar butt flush against it (pt-16 / top-16).
          The account chip is the CTA; the marketing nav stays for a way back out. */}
      <Header
        nav={CONDUCTOR_NAV}
        variant="compact"
        cta={<AccountChip className="hidden items-center sm:flex" />}
        mobileCta={<AccountChip className="flex w-full justify-center" />}
      />
      <div className="flex min-h-screen bg-background pt-16">
        <Sidebar />
        <main className="min-w-0 flex-1">
          <SystemBanner />
          {children}
        </main>
      </div>
    </>
  );
}

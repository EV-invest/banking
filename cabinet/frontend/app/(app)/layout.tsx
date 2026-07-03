import type { ReactNode } from "react";

import { Header } from "@evinvest/uikit";

import { CONDUCTOR_NAV } from "@/application/layout/conductor-nav";
import { Sidebar } from "@/application/layout/sidebar";
import { SystemBanner } from "@/application/layout/system-banner";

// The authenticated app shell: the shared EV brand header (one shell across all
// surfaces — nav links lead back to the conductor's pages as hard cross-zone
// links; plain <a> is the uikit default) over a persistent left rail beside a
// scrollable content column. No footer here by design. The pt clears the fixed
// header's unscrolled height. Auth is enforced upstream in `proxy.ts` —
// unauthenticated requests are redirected to /login before this layout renders.
// The system banner (maintenance · read-only · announcement) mounts once here;
// (auth) pages have no session, so they are intentionally excluded.
export default function AppLayout({ children }: { children: ReactNode }) {
  return (
    <>
      {/* Forced opaque: unlike the conductor's hero, app content sits directly
          under the bar, so the 0–50px transparent scroll state would overlap. */}
      <Header nav={CONDUCTOR_NAV} className="bg-main-black/90 backdrop-blur-md border-main-mist/10" />
      <div className="flex min-h-screen bg-background pt-24">
        <Sidebar />
        <main className="min-w-0 flex-1">
          <SystemBanner />
          {children}
        </main>
      </div>
    </>
  );
}

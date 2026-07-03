import type { ReactNode } from "react";

import { Header } from "@evinvest/uikit";

import { CONDUCTOR_NAV } from "@/application/layout/conductor-nav";
import { Sidebar } from "@/application/layout/sidebar";

// The authenticated app shell: the shared EV brand header (one shell across all
// surfaces — nav links lead back to the conductor's pages as hard cross-zone
// links; plain <a> is the uikit default) over a persistent left rail beside a
// scrollable content column. No footer here by design. The pt clears the fixed
// header's unscrolled height. Auth is enforced upstream in `proxy.ts` —
// unauthenticated requests are redirected to /login before this layout renders.
export default function AppLayout({ children }: { children: ReactNode }) {
  return (
    <>
      <Header nav={CONDUCTOR_NAV} />
      <div className="flex min-h-screen bg-background pt-24">
        <Sidebar />
        <main className="min-w-0 flex-1">{children}</main>
      </div>
    </>
  );
}

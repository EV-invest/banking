import type { ReactNode } from "react";

import { Sidebar } from "@/application/layout/sidebar";

// The authenticated app shell: a persistent left rail (Fund · Products · Settings) beside
// a scrollable content column. Auth is enforced upstream in `proxy.ts` — unauthenticated
// requests are redirected to /login before they ever reach this layout.
export default function AppLayout({ children }: { children: ReactNode }) {
  return (
    <div className="flex min-h-screen bg-background">
      <Sidebar />
      <main className="min-w-0 flex-1">{children}</main>
    </div>
  );
}

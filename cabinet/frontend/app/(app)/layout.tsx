import type { ReactNode } from "react";

import { Sidebar } from "@/application/layout/sidebar";
import { SystemBanner } from "@/application/layout/system-banner";

// The authenticated app shell: a fixed left rail beside a scrollable content
// column. Chromeless by design — the brand header is conductor-owned and
// injected at the zone mount (the cabinet knows nothing about the outer site;
// `--ev-shell-offset` is the only shell contract). The rail is `fixed` (see
// Sidebar), so the content column reserves its width with a matching left
// padding — both read the shared `--cabinet-rail-w` token. No footer here by
// design. Auth is enforced upstream in `proxy.ts` — unauthenticated requests
// are redirected to /login before this layout renders. The system banner
// (maintenance · read-only · announcement) mounts once here; (auth) pages have
// no session, so they are intentionally excluded.
export default function AppLayout({ children }: { children: ReactNode }) {
  return (
    <div className="flex min-h-[calc(100dvh-var(--ev-shell-offset,0px))] bg-background pl-[var(--cabinet-rail-w)]">
      <Sidebar />
      <main className="min-w-0 flex-1">
        <SystemBanner />
        {children}
      </main>
    </div>
  );
}

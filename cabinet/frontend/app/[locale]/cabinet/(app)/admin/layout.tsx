"use client";

import { type ReactNode } from "react";

import { isPanelAccessRoute } from "@/entities/scope/lib/access";
import { useCabinetPathname } from "@/shared/lib/cabinet-route";
import { useSession } from "@/shared/lib/use-session";
import { ForbiddenScreen } from "@/shared/ui/forbidden-screen";

// Client-side guard for the admin console. This is cosmetic defense in depth — the
// BFF admin routes are the real boundary (they re-check the role and return 403),
// so a manually-crafted request never reaches operator data regardless of what the
// browser renders.
//
// It used to `router.replace()` a non-operator back to the dashboard, which reads
// as the app losing the click: the URL they typed or were sent silently becomes a
// different page and nothing says why. Show the 403 instead — same surface as
// `forbidden.tsx`, which is the server half of this rule and cannot be used here
// (the principal comes from the shell's `/api/auth/session`, read in the browser,
// so this component is a Client Component and `forbidden()` is server-only).
//
// `session === null` is "not resolved yet", not "denied": rendering the 403 while
// the fetch is in flight would flash it at every operator on every admin load.
//
// One page is let through without a console role: an allocation's panel-access page,
// which a scope's own admin — usually no operator at all — has to reach. It gates itself
// on the scope, and the identity plane re-checks every call it makes.
export default function AdminLayout({ children }: { children: ReactNode }) {
  const session = useSession();
  const pathname = useCabinetPathname();
  const denied = session !== null && !session.user?.isAdmin && !isPanelAccessRoute(pathname);

  if (denied) return <ForbiddenScreen />;
  return <>{children}</>;
}

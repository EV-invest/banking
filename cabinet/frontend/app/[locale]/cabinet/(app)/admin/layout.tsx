"use client";

import { useRouter } from "next/navigation";
import { type ReactNode, useEffect } from "react";

import { useSession } from "@/shared/lib/use-session";

// Client-side guard for the admin console: bounce a non-operator session back to the
// dashboard. This is cosmetic defense in depth — the BFF admin routes are the real
// boundary (they re-check the role and return 403), so a manually-crafted request
// never reaches operator data regardless of what the browser renders.
export default function AdminLayout({ children }: { children: ReactNode }) {
  const session = useSession();
  const router = useRouter();
  const denied = session !== null && !session.user?.isAdmin;

  useEffect(() => {
    if (denied) router.replace("/");
  }, [denied, router]);

  if (denied) return null;
  return <>{children}</>;
}

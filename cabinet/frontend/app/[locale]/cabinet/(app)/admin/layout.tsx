"use client";

import { type ReactNode } from "react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { StatusScreen } from "@evinvest/uikit";
import { localePath } from "@evinvest/i18n";

import { cabinetPath } from "@/shared/config/base-path";
import { useSession } from "@/shared/lib/use-session";

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
export default function AdminLayout({ children }: { children: ReactNode }) {
  const session = useSession();
  const t = useT();
  const locale = useLocale();
  const denied = session !== null && !session.user?.isAdmin;

  if (denied) {
    return (
      <StatusScreen
        accent="gold"
        code="403"
        eyebrow={t("status.forbidden.eyebrow")}
        headlineLead={t("status.forbidden.headlineLead")}
        headlineAccent={t("status.forbidden.headlineAccent")}
        subtext={t("status.forbidden.subtext")}
        links={[
          { label: t("status.backHome"), href: cabinetPath(locale, "/"), leadingArrow: true },
          {
            label: t("status.requestAccess"),
            href: localePath(locale, "/contact"),
            variant: "outline",
          },
        ]}
      />
    );
  }
  return <>{children}</>;
}

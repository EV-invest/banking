"use client";

import { useLocale, useT } from "@evinvest/i18n/react";
import { StatusScreen } from "@evinvest/uikit";
import { localePath } from "@evinvest/i18n";

import { cabinetPath } from "@/shared/config/base-path";

// The client-side 403: the same surface as `forbidden.tsx`, which is the server half of this
// rule and cannot be used where the principal is only known in the browser. Shared by the
// admin console's guard and the one admin page a scope admin may open, so the two refusals
// read alike.
export function ForbiddenScreen() {
  const t = useT();
  const locale = useLocale();
  return (
    <StatusScreen
      accent="warn"
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

"use client";

// The desktop Security pane: how the account signs in, with the live session count and the
// way into the session list.

import { useT } from "@evinvest/i18n/react";

import { Button } from "@evinvest/uikit";

import { SignInMethodRows } from "@/features/auth/ui/sign-in-methods";

import type { Session } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { CARD, Hairline, Row, RowLabel } from "@/shared/ui/list-card";
import { SectionHeader } from "@/views/settings/ui/fields";

export function SecuritySection({ sessions, onManageSessions }: { sessions: Session[] | undefined; onManageSessions: () => void }) {
  const t = useT();
  const count = sessions?.length;
  // One plural message rather than the two hand-written branches this replaced: a locale
  // with more than two plural forms cannot be spelled with an `=== 1` ternary.
  const summary = count === undefined ? t("settings.sessionsLoading", "Loading active sessions…") : t("settings.devicesSignedIn", "{n, plural, one {# device currently signed in} other {# devices currently signed in}}", { n: count });
  return (
    <section className={cn(CARD, "px-6 py-5.5")}>
      <SectionHeader title={t("ui.security", "Security")} sub={t("settings.securitySub", "How you sign in and where your account is active")} />
      <SignInMethodRows />
      <Hairline />
      <Row>
        <RowLabel title={t("ui.sessionsDevices", "Sessions & devices")} sub={summary} />
        {/* i18n-max: 11 — a `shrink-0` Button beside the `min-w-0 flex-1` row label. */}
        <Button variant="outline" size="sm" className="border-border" onClick={onManageSessions}>
          {t("ui.manage", "Manage")}
        </Button>
      </Row>
    </section>
  );
}

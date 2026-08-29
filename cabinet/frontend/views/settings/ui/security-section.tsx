"use client";

// The desktop Security pane: the real auth model — sign-in is Google-managed — with the
// live session count and the way into the session list.

import { useT } from "@evinvest/i18n/react";

import { Badge, Button, Skeleton } from "@evinvest/uikit";

import type { Session } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { CARD, Hairline, Row, RowLabel } from "@/shared/ui/list-card";
import { formatEmail } from "@/views/settings/lib/contact";
import { SectionHeader } from "@/views/settings/ui/fields";

export function SecuritySection({
  email,
  loading,
  sessions,
  onManageSessions,
}: {
  email: string | null;
  loading: boolean;
  sessions: Session[] | undefined;
  onManageSessions: () => void;
}) {
  const t = useT();
  const count = sessions?.length;
  const summary = count === undefined ? "Loading active sessions…" : count === 1 ? "1 device currently signed in" : `${count} devices currently signed in`;
  return (
    <section className={cn(CARD, "px-6 py-5.5")}>
      <SectionHeader title={t("ui.security")} sub="How you sign in and where your account is active" />
      <div className="flex items-center gap-3 rounded-xl border border-border bg-main-surface px-4 py-3.5">
        {/* Google's mark is only licensed on a white plate, so this one square stays off-theme. */}
        <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-white">
          <GoogleMark />
        </span>
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium text-foreground">Signed in with Google</p>
          {loading ? <Skeleton className="mt-1 h-3.5 w-44" /> : <p className="truncate text-xs text-muted-foreground">{formatEmail(email) || "—"}</p>}
        </div>
        <Badge className="border-transparent bg-main-accent-t1/15 text-main-accent-t1">Connected</Badge>
      </div>
      <p className="mb-1 mt-3 text-sm leading-relaxed text-muted-foreground">
        Your sign-in and password are managed by Google. Two-factor authentication and recovery are configured in your Google Account.
      </p>
      <Hairline />
      <Row>
        <RowLabel title={t("ui.sessionsDevices")} sub={summary} />
        <Button variant="outline" size="sm" className="border-border" onClick={onManageSessions}>
          Manage
        </Button>
      </Row>
    </section>
  );
}

// Google's four brand hexes are fixed by their identity guidelines and are passed to the
// SVG `fill` attribute, which takes a value rather than a class — so they stay literal and
// deliberately do not follow the theme.
function GoogleMark() {
  return (
    <svg viewBox="0 0 24 24" className="size-4.5" aria-hidden="true">
      <path fill="#4285F4" d="M23.52 12.27c0-.82-.07-1.6-.2-2.36H12v4.47h6.47a5.53 5.53 0 0 1-2.4 3.63v3h3.88c2.27-2.09 3.57-5.17 3.57-8.74Z" />
      <path fill="#34A853" d="M12 24c3.24 0 5.96-1.08 7.95-2.91l-3.88-3.01c-1.08.72-2.45 1.15-4.07 1.15-3.13 0-5.78-2.11-6.73-4.96H1.27v3.1A12 12 0 0 0 12 24Z" />
      <path fill="#FBBC05" d="M5.27 14.27a7.2 7.2 0 0 1 0-4.54v-3.1H1.27a12 12 0 0 0 0 10.74l4-3.1Z" />
      <path fill="#EA4335" d="M12 4.77c1.76 0 3.35.61 4.6 1.8l3.43-3.43A11.97 11.97 0 0 0 12 0 12 12 0 0 0 1.27 6.63l4 3.1C6.22 6.88 8.87 4.77 12 4.77Z" />
    </svg>
  );
}

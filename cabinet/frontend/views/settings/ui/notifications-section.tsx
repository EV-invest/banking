"use client";

// Notifications: the real delivery-preference store. One section for both breakpoints —
// the desktop rail renders it in place, the mobile stack pushes it as its own screen.

import { useT } from "@evinvest/i18n/react";

import { useState } from "react";

import { Skeleton, Switch } from "@evinvest/uikit";

import {
  notificationSettingsResource,
  setChannelEnabled,
  setTopicSubscription,
} from "@/entities/notification/model/notification-resource";
import { refreshUnreadCount } from "@/entities/notification/model/notification-store";
import type { NotificationSettings } from "@/shared/contracts/notifications";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { Hairline, ListCard, ListCardTitle, Row, RowLabel } from "@/shared/ui/list-card";

/**
 * Delivery preferences. Both master channels are opt-out and may be off at once —
 * "stop contacting me" is a supported state, so nothing here keeps one of them on.
 *
 * Every write returns the full snapshot, so state is replaced from the response
 * rather than patched locally; that keeps the per-topic email toggles honest when
 * the master email switch turns them all moot.
 */
export function NotificationsSection() {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [writeError, setWriteError] = useState<string | null>(null);

  // Every write answers with the whole new matrix and publishes it into the cache, so the
  // toggles below stay in step without this section holding a second copy of the state.
  const read = useResource(notificationSettingsResource);
  const settings = read.data ?? null;
  // Only a read that has actually failed reports — while it is still in flight there is
  // nothing wrong, and the skeleton switches below are the right thing to show.
  const error = writeError ?? (settings || !read.error ? null : (read.error.message || "could not load notification settings"));

  async function run(fn: () => Promise<NotificationSettings>) {
    setBusy(true);
    setWriteError(null);
    try {
      await fn();
      // Switching the in-app channel changes what the badge should read, and the
      // sidebar has no other reason to refetch.
      void refreshUnreadCount();
    } catch (e) {
      setWriteError(e instanceof Error ? e.message : "could not save");
    } finally {
      setBusy(false);
    }
  }

  if (error && !settings) {
    return <p className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</p>;
  }

  return (
    <div className="flex flex-col gap-4 lg:gap-4.5">
      <ListCard className="lg:px-5.5">
        <ListCardTitle sub="Choose where notifications reach you. Both can be off.">Delivery</ListCardTitle>
        <Hairline />
        <Row>
          <RowLabel title={t("ui.inYourCabinet")} sub="On by default. Turn this off and notifications stop appearing in your cabinet." />
          {settings ? (
            <Switch
              checked={settings.in_app_enabled}
              disabled={busy}
              onCheckedChange={(v) => void run(() => setChannelEnabled("in_app", v))}
              aria-label="In-app notifications"
            />
          ) : (
            <Skeleton className="h-5 w-9 shrink-0 rounded-full" />
          )}
        </Row>
        <Hairline />
        <Row>
          <RowLabel
            title={t("ui.email")}
            sub={settings ? (settings.email_verified ? `Sent to ${settings.email} · verified` : `${settings.email} · unverified, so email is not sent`) : undefined}
          />
          {settings ? (
            <Switch
              checked={settings.email_enabled}
              disabled={busy || !settings.email_verified}
              onCheckedChange={(v) => void run(() => setChannelEnabled("email", v))}
              aria-label="Email notifications"
            />
          ) : (
            <Skeleton className="h-5 w-9 shrink-0 rounded-full" />
          )}
        </Row>
      </ListCard>

      <ListCard className="lg:px-5.5">
        <div className="flex items-start justify-between gap-4">
          <ListCardTitle sub="Email a copy for these. Unsubscribing is one click — no confirmation step.">What you follow</ListCardTitle>
          <p className="shrink-0 pt-3 text-xs font-semibold uppercase tracking-widest text-muted-foreground">Email</p>
        </div>
        {settings
          ? settings.topics.map((t) => (
              <div key={t.topic}>
                <Hairline />
                {/* Wraps rather than switching at a breakpoint: the controls drop under
                    the label only when they genuinely do not fit, so the row is correct at
                    every width instead of at two. The `sm:` variants this replaced were
                    rendering as a permanent centred column — see the PR for detail. */}
                <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2.5 py-3.5">
                  <RowLabel title={t.label} sub={t.description} />
                  <div className="flex shrink-0 items-center gap-3">
                    {/* Kept hand-written: at 28px it is shorter than uikit's smallest Button, and
                        growing it would push the switch beside it out of the row. */}
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void run(() => setTopicSubscription(t.topic, !t.subscribed, t.email_enabled))}
                      className={cn(
                        "rounded-lg px-3 py-1.5 text-xs font-medium outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-40",
                        t.subscribed ? "border border-border/60 text-foreground hover:bg-foreground/5" : "border border-main-accent-t1/50 text-main-accent-t1 hover:bg-main-accent-t1/10",
                      )}
                    >
                      {t.subscribed ? "Following" : "Follow"}
                    </button>
                    <Switch
                      checked={t.subscribed && t.email_enabled && settings.email_enabled}
                      disabled={busy || !t.subscribed || !settings.email_enabled}
                      onCheckedChange={(v) => void run(() => setTopicSubscription(t.topic, true, v))}
                      aria-label={`Email copy for ${t.label}`}
                    />
                  </div>
                </div>
              </div>
            ))
          : [0, 1, 2].map((i) => (
              <div key={i}>
                <Hairline />
                <Row>
                  <Skeleton className="h-4 w-32" />
                  <Skeleton className="h-5 w-9 shrink-0 rounded-full" />
                </Row>
              </div>
            ))}
      </ListCard>

      {error && <p className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</p>}
      <p className="text-xs leading-relaxed text-muted-foreground">
        Turn a channel off and we stop sending on it. Turn both off and we stop notifying you altogether — updates still live on the fund pages whenever you want them.
      </p>
    </div>
  );
}

"use client";

// Notifications: the real delivery-preference store. One section for both breakpoints —
// the desktop rail renders it in place, the mobile stack pushes it as its own screen.

import { useT } from "@evinvest/i18n/react";

import { useState } from "react";

import { Skeleton, Switch, Toggle } from "@evinvest/uikit";

import {
  notificationSettingsResource,
  setChannelEnabled,
  setTopicSubscription,
} from "@/entities/notification/model/notification-resource";
import { refreshUnreadCount } from "@/entities/notification/model/notification-store";
import type { NotificationSettings } from "@/shared/contracts/notifications";
import { errorMessage } from "@/shared/lib/api-client";
import { useResource } from "@/shared/lib/resource";
import { Hairline, ListCard, ListCardTitle, Row, RowLabel } from "@/shared/ui/list-card";
import { SectionLabel } from "@/shared/ui/page-frame";

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
  const error = writeError ?? (settings || !read.error ? null : errorMessage(read.error, t));

  async function run(fn: () => Promise<NotificationSettings>) {
    setBusy(true);
    setWriteError(null);
    try {
      await fn();
      // Switching the in-app channel changes what the badge should read, and the
      // sidebar has no other reason to refetch.
      void refreshUnreadCount();
    } catch (e) {
      setWriteError(errorMessage(e, t));
    } finally {
      setBusy(false);
    }
  }

  if (error && !settings) {
    return <p className="rounded-md border border-accent-error/40 bg-accent-error/10 px-3 py-2 text-sm text-accent-error">{error}</p>;
  }

  return (
    <div className="flex flex-col gap-4 lg:gap-4.5">
      <ListCard className="lg:px-5.5">
        <ListCardTitle sub={t("notif.deliverySub")}>{t("notif.delivery")}</ListCardTitle>
        <Hairline />
        <Row>
          <RowLabel title={t("ui.inYourCabinet")} sub={t("notif.inAppSub")} />
          {settings ? (
            <Switch
              checked={settings.in_app_enabled}
              disabled={busy}
              onCheckedChange={(v) => void run(() => setChannelEnabled("in_app", v))}
              aria-label={t("notif.a11y.inApp")}
            />
          ) : (
            <Skeleton className="h-5 w-9 shrink-0 rounded-full" />
          )}
        </Row>
        <Hairline />
        <Row>
          <RowLabel
            title={t("ui.email")}
            sub={settings ? t(settings.email_verified ? "notif.emailVerified" : "notif.emailUnverified", { email: settings.email }) : undefined}
          />
          {settings ? (
            <Switch
              checked={settings.email_enabled}
              disabled={busy || !settings.email_verified}
              onCheckedChange={(v) => void run(() => setChannelEnabled("email", v))}
              aria-label={t("notif.a11y.email")}
            />
          ) : (
            <Skeleton className="h-5 w-9 shrink-0 rounded-full" />
          )}
        </Row>
      </ListCard>

      <ListCard className="lg:px-5.5">
        <div className="flex items-start justify-between gap-4">
          <ListCardTitle sub={t("notif.topicsSub")}>{t("notif.whatYouFollow")}</ListCardTitle>
          {/* i18n-max: 12 — `shrink-0` uppercase column header with `tracking-widest`. */}
          <SectionLabel className="shrink-0 pt-3">{t("ui.email")}</SectionLabel>
        </div>
        {settings
          ? // Named `topic`, not `t`: the translator is bound above and a `t` here would
            // shadow it silently — every `t(...)` inside the loop would be a topic row.
            settings.topics.map((topic) => (
              <div key={topic.topic}>
                <Hairline />
                {/* Wraps rather than switching at a breakpoint: the controls drop under
                    the label only when they genuinely do not fit, so the row is correct at
                    every width instead of at two. The `sm:` variants this replaced were
                    rendering as a permanent centred column — see the PR for detail. */}
                <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2.5 py-3.5">
                  {/* Topic label and description are server data — they arrive already
                      worded from `NotificationSettings.topics` and render verbatim, so
                      they stay English until the hub localises them. */}
                  <RowLabel title={topic.label} sub={topic.description} />
                  <div className="flex shrink-0 items-center gap-3">
                    {/* A pressed/unpressed pair rather than a Button: "Following" is a state the
                        reader is in, and the toggle's `aria-pressed` says so. The `xs` size is
                        the 28px that keeps the switch beside it on one row. */}
                    {/* i18n-max: 12 — the row wraps rather than clips, but a longer label
                        drops the controls onto their own line on every phone. */}
                    <Toggle
                      variant="outline"
                      size="xs"
                      pressed={topic.subscribed}
                      disabled={busy}
                      onPressedChange={(pressed) => void run(() => setTopicSubscription(topic.topic, pressed, topic.email_enabled))}
                      className="px-3 text-xs"
                    >
                      {topic.subscribed ? t("notif.following") : t("notif.follow")}
                    </Toggle>
                    <Switch
                      checked={topic.subscribed && topic.email_enabled && settings.email_enabled}
                      disabled={busy || !topic.subscribed || !settings.email_enabled}
                      onCheckedChange={(v) => void run(() => setTopicSubscription(topic.topic, true, v))}
                      aria-label={t("notif.a11y.emailCopyFor", { topic: topic.label })}
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

      {error && <p className="rounded-md border border-accent-error/40 bg-accent-error/10 px-3 py-2 text-sm text-accent-error">{error}</p>}
      <p className="text-xs leading-relaxed text-ink-soft">{t("notif.channelsFootnote")}</p>
    </div>
  );
}

"use client";

import { useLocale, useT } from "@evinvest/i18n/react";
import { useCallback } from "react";

import { BadgeCheck } from "lucide-react";

import { Button, Card, Skeleton } from "@evinvest/uikit";

import { positionsResource } from "@/entities/fund/model/fund-resource";
import { operationsResource } from "@/entities/operation/model/operation-resource";
import { sessionsResource } from "@/entities/session/model/session-resource";
import { profileResource } from "@/entities/user/model/profile-resource";
import { walletResource } from "@/entities/wallet/model/wallet-resource";
import { errorMessage } from "@/shared/lib/api-client";
import { cn } from "@/shared/lib/cn";
import { formatDay } from "@/shared/lib/datetime";
import { formatSignedUsd, formatUsd, num } from "@/shared/lib/money";
import { useResource } from "@/shared/lib/resource";
import { Link } from "@/shared/ui/cabinet-link";
import { CARD, InitialsAvatar, Pill } from "@/shared/ui/list-card";
import { MobileAppBar } from "@/shared/ui/mobile-appbar";
import { SECTION_STAGGER, Stagger, StaggerItem } from "@/shared/ui/motion";
import { formatCount, STAT_STRIP, StatDivider, StatTile } from "@/shared/ui/stat-tile";
import { seconds } from "@/views/operations/lib/format";
import { displayName, enumLabel, initialsOfName, statusTone, truncateName } from "@/views/profile/lib/format";
import { ActivityCard, PersonalCard, SecurityCard, VerificationCard } from "@/views/profile/ui/profile-cards";

// The investor profile: who the reader is on the platform and how their account stands.
// Read-only by design — it used to be an editor as well, which made it a second, worse
// Settings and left nobody sure where a detail was changed. Every editable fact here
// links to the settings section that owns it (`/settings?section=…`).
//
// Two Figma frames, one component: `cabinet/mobile/profile` (node 503:266) below `lg` — a
// hero card over stacked cards — and `cabinet/profile` (node 489:258) above it, the hero
// and a stat strip over two columns. Name/email and the identity fields are real (core
// UsersService via the BFF), as are Identity verification (the admin-managed
// `status`/`kyc_level`/`email_verified`), the figures (fund positions and the wallet) and
// the activity counts (the same timeline `/operations` lists). The mock's per-document KYC
// rows have no document store behind them, so this states the KYC level the hub actually
// holds instead of inventing four.

export function ProfileView() {
  const t = useT();
  const locale = useLocale();
  // Bound once per locale, as on the dashboard: AnimatedNumber restarts its count whenever
  // the identity of `format` changes.
  const usd = useCallback((n: number) => formatUsd(n, locale), [locale]);
  const signedUsd = useCallback((n: number) => formatSignedUsd(n, locale), [locale]);

  // Every read is cached and shared with another screen — the profile with the account
  // chip and Settings, the rest with Home, Operations and Settings — and all five are
  // warmed on intent (application/prefetch.ts), so arriving here fills the page on the
  // first frame.
  const { data: profile, error: readError, isLoading: loading } = useResource(profileResource);
  const positions = useResource(positionsResource);
  const wallet = useResource(walletResource);
  const operations = useResource(operationsResource, undefined);
  const sessionList = useResource(sessionsResource);
  // One banner, the first read that failed. A failed figure is a dash in its tile, not a
  // zero — see the strip below.
  const posFailed = !positions.data && !!positions.error;
  const walletFailed = !wallet.data && !!wallet.error;
  const failed = (profile || !readError ? null : readError) ?? (posFailed ? positions.error : null) ?? (walletFailed ? wallet.error : null);
  const error = failed ? errorMessage(failed, t) : null;

  const email = profile?.email ?? "";
  const legalName = (profile?.legal_name ?? "").trim();
  const name = truncateName(legalName) || (loading ? "…" : displayName(email, t));

  const pos = positions.data?.positions ?? [];
  const value = pos.reduce((s, p) => s + num(p.value), 0);
  const pnl = pos.reduce((s, p) => s + num(p.pnl), 0);
  const ops = operations.data?.operations ?? [];
  // The hub caps the list (DEFAULT_PAGE): when it did, the oldest row here is the
  // hundredth operation, not the first, and "active since" would be a lie — so it is
  // only shown over a complete list. The Activity card says the same in its caption.
  const truncated = operations.data?.truncated ?? false;
  // The oldest stamped operation is when the account first moved money — "active since"
  // is that, not the day the account was created, which the hub does not expose.
  const oldest = truncated
    ? 0
    : ops.reduce((min, op) => {
        const at = seconds(op.created_at);
        return at > 0 && (min === 0 || at < min) ? at : min;
      }, 0);

  // The one action on the page. Desktop keeps it in the heading, mobile in the hero,
  // full-width — both lead to the section that edits what the hero shows.
  const edit = (className: string) => (
    // i18n-max: 20 — a `shrink-0` outline Button beside the `min-w-0` heading column.
    <Button asChild variant="outline" className={cn("border-border", className)}>
      <Link href="/settings?section=personal">{t("profile.editInSettings")}</Link>
    </Button>
  );
  const personal = <PersonalCard loading={loading} profile={profile ?? null} email={email} />;
  const activity = <ActivityCard loading={operations.isLoading} operations={ops} truncated={truncated} error={operations.data || !operations.error ? null : errorMessage(operations.error, t)} />;
  const verification = <VerificationCard loading={loading} profile={profile ?? null} email={email} />;
  const security = <SecurityCard loading={loading} email={email} sessions={sessionList.data} sessionsFailed={!sessionList.data && !!sessionList.error} />;

  return (
    <>
      <MobileAppBar title={t("ui.profile")} backHref="/settings" />

      <Stagger delay={SECTION_STAGGER} step={SECTION_STAGGER} className="flex flex-col gap-4 px-5 pb-6 pt-4.5 lg:gap-5 lg:px-8 lg:pb-8 lg:pt-6">
        {/* Desktop page heading — the mobile app bar owns this below `lg`. */}
        <StaggerItem className="hidden items-center justify-between gap-4 lg:flex">
          <div className="min-w-0">
            <h1 className="text-2xl font-semibold text-ink">{t("ui.profile")}</h1>
            <p className="text-sm text-ink-soft">{t("profile.subtitle")}</p>
          </div>
          {edit("shrink-0")}
        </StaggerItem>

        {error && (
          <StaggerItem as="p" className="rounded-md border border-accent-error/40 bg-accent-error/10 px-3 py-2 text-sm text-accent-error">
            {error}
          </StaggerItem>
        )}

        {/* card-Hero — centred on mobile (Figma 503:274), a wide chip on desktop (489:258). */}
        <StaggerItem className={cn(CARD, "flex flex-col items-center gap-3 px-5 pb-5.5 pt-6 lg:flex-row lg:gap-5 lg:px-6 lg:py-5.5")}>
          <InitialsAvatar initials={initialsOfName(name, email)} className="size-16 text-2xl lg:text-xl" />
          <div className="flex min-w-0 flex-1 flex-col items-center gap-1 text-center lg:items-start lg:text-left">
            <div className="flex min-w-0 flex-col items-center gap-1 lg:flex-row lg:items-baseline lg:gap-3">
              {loading ? <Skeleton className="h-6 w-40" /> : <p className="truncate text-lg font-semibold text-ink lg:text-xl">{name || t("ui.account")}</p>}
              {loading ? <Skeleton className="h-4 w-48" /> : <p className="truncate text-sm text-ink-soft">{email || t("auth.notSignedIn")}</p>}
            </div>
            {!loading && (
              // i18n-max: 12 per Pill — they sit beside the truncated display name.
              <div className="mt-1 flex flex-wrap items-center justify-center gap-2 lg:justify-start">
                {profile?.email_verified && <Pill icon={BadgeCheck}>{t("ui.verified")}</Pill>}
                {profile?.status && <Pill tone={statusTone(profile.status)}>{enumLabel("admin.status", profile.status, t)}</Pill>}
                {profile?.kyc_level !== undefined && <Pill tone="neutral">{t("profile.kycLevelPill", { n: profile.kyc_level })}</Pill>}
                {profile?.role && <Pill tone="neutral">{enumLabel("admin.role", profile.role, t)}</Pill>}
              </div>
            )}
            {oldest > 0 && <p className="mt-1 text-xs text-ink-soft">{t("profile.activeSince", { date: formatDay(String(oldest), locale) })}</p>}
          </div>
          {/* Mobile puts the action in the hero; desktop has it in the page heading. */}
          {edit("w-full lg:hidden")}
        </StaggerItem>

        {/* Stat strip — the same tiles and arrangement as Home, so a figure reads the
            same on both screens: a 2×2 card grid on mobile, one divided strip from `lg`. */}
        <StaggerItem as={Card} className={cn(STAT_STRIP, "rounded-none border-0 bg-transparent py-0 shadow-none lg:rounded-xl lg:border lg:bg-card lg:py-5 lg:shadow-sm")}>
          <StatTile label={t("dash.portfolioValue")} value={positions.isLoading ? null : value} format={usd} hint={t("profile.hintAtNav")} unavailable={posFailed} />
          <StatDivider />
          <StatTile label={t("dash.unrealizedPnl")} value={positions.isLoading ? null : pnl} format={signedUsd} tone={pnl < 0 ? "loss" : pnl > 0 ? "gain" : undefined} hint={t("dash.hintAcrossPositions")} tip="dashboard.stats.unrealized-pnl" unavailable={posFailed} />
          <StatDivider />
          <StatTile label={t("dash.available")} value={wallet.isLoading ? null : num(wallet.data?.balance?.available)} format={usd} hint={t("dash.hintAutoDeploysEod")} tip="dashboard.stats.available" unavailable={walletFailed} />
          <StatDivider />
          <StatTile label={t("dash.activeStrategies")} value={positions.isLoading ? null : pos.length} format={formatCount} hint={t("dash.hintFundPositions")} unavailable={posFailed} />
        </StaggerItem>

        {/* ── Mobile (Figma cabinet/mobile/profile) ────────────────────────── */}
        {/* One item per viewport block rather than per card: the four cards are the same
            elements rendered into both blocks, and giving each a place in the sequence
            would have them animating twice over. */}
        <StaggerItem className="flex flex-col gap-4 lg:hidden">
          {verification}
          {personal}
          {activity}
          {security}
        </StaggerItem>

        {/* ── Desktop (Figma cabinet/profile) ──────────────────────────────── */}
        <StaggerItem className="hidden items-start gap-5 lg:flex">
          <div className="flex min-w-0 flex-1 flex-col gap-5">
            {personal}
            {activity}
          </div>
          <div className="flex w-97 shrink-0 flex-col gap-5">
            {verification}
            {security}
          </div>
        </StaggerItem>
      </Stagger>
    </>
  );
}

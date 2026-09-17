"use client";

// The profile's cards. Every one is read-only — the profile is a statement of who the
// reader is and how their account stands, and each card that shows an editable fact
// links to the settings section that edits it, rather than editing in place.

import { useLocale, useT } from "@evinvest/i18n/react";

import { BadgeCheck } from "lucide-react";
import type { ReactNode } from "react";

import { Button, Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle, Skeleton } from "@evinvest/uikit";

import { StartVerificationRow } from "@/features/kyc";
import { isUnverified } from "@/entities/user/lib/kyc";
import type { Operation, Session, UserProfile } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { TipAnchor } from "@/shared/tips";
import { Link } from "@/shared/ui/cabinet-link";
import { Hairline, ListCard, ListCardTitle, Pill, Row, RowLabel, RowValue } from "@/shared/ui/list-card";
import { enumLabel, statusTone } from "@/views/profile/lib/format";
import { dayLabel, kindLabel, seconds } from "@/views/operations/lib/format";
import { formatPhone } from "@/views/settings/lib/contact";
import { PERSONAL } from "@/views/settings/lib/sections";

// The text links that lead out of a card — hand-written, so they carry their own focus ring.
const TEXT_LINK = "shrink-0 rounded-sm text-xs font-medium text-primary-ink outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring";

// uikit's Empty draws a dashed frame but leaves the border width to the caller, and doubles
// its padding at `md`; this one sits inside a card, not on a page of its own.
const EMPTY_BOX = "border md:p-6";

/** A card whose title row carries a link to where the facts on it are changed. */
function TitledCard({ title, sub, action, className, children }: { title: string; sub: string; action?: ReactNode; className?: string; children: ReactNode }) {
  return (
    <ListCard className={className}>
      <div className="flex items-start justify-between gap-3">
        <ListCardTitle sub={sub}>{title}</ListCardTitle>
        {action && <div className="pt-3">{action}</div>}
      </div>
      <Hairline />
      {children}
    </ListCard>
  );
}

/** A label/value pair: stacked on mobile, two columns from `lg`. Values are text, never inputs. */
function FactRow({ label, value, loading }: { label: string; value: string | undefined; loading: boolean }) {
  const v = (value ?? "").trim();
  return (
    <div className="flex min-w-0 flex-col gap-1 py-3 lg:flex-row lg:items-baseline lg:justify-between lg:gap-4 lg:py-3.5">
      <span className="text-xs font-medium text-ink-soft lg:shrink-0 lg:text-sm lg:font-normal">{label}</span>
      {loading ? <Skeleton className="h-5 w-40" /> : <span className={cn("min-w-0 break-words text-sm font-medium lg:text-right", v ? "text-ink" : "text-ink-soft")}>{v || "—"}</span>}
    </div>
  );
}

// How many of the seven identity fields may be blank before the card says so. Three is
// where a record stops being "a detail missing" and becomes one that statements and
// withdrawals cannot be produced from.
const INCOMPLETE_AT = 3;

/** `card-Personal` — the identity record as the fund holds it, and where to change it. */
export function PersonalCard({ loading, profile, email, className }: { loading: boolean; profile: UserProfile | null; email: string; className?: string }) {
  const t = useT();
  const blanks = PERSONAL.filter((f) => !(profile?.[f.key] ?? "").trim()).length;
  return (
    <TitledCard
      title={t("settings.nav.personal")}
      sub={t("settings.personalSub")}
      className={className}
      action={
        // i18n-max: 10 — a `shrink-0` text link beside the `min-w-0` title column.
        <Link href="/settings?section=personal" className={TEXT_LINK}>
          {t("profile.edit")}
        </Link>
      }
    >
      {PERSONAL.map((field, i) => (
        <div key={field.key}>
          {i > 0 && <Hairline />}
          <FactRow label={t(field.labelKey)} value={field.key === "phone" ? formatPhone(profile?.phone ?? "") : profile?.[field.key]} loading={loading} />
        </div>
      ))}
      <Hairline />
      <FactRow label={t("ui.emailAddress")} value={email} loading={loading} />
      {/* The zero state: a mostly blank record is the one every new account starts with,
          and the sentence says what the blanks cost rather than just that they exist. */}
      {!loading && blanks >= INCOMPLETE_AT && (
        <>
          <Hairline />
          <Link href="/settings?section=personal" className="block rounded-md py-3 text-xs leading-snug text-ink-soft outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
            {t("profile.completeDetails")}
          </Link>
        </>
      )}
    </TitledCard>
  );
}

// The four kinds the card counts, in the order the operations screen's filter row lists
// them. Fees are left out: they are charged, not done, and the card is about what the
// reader did.
const COUNTED = ["deposit", "withdrawal", "subscription", "redemption"] as const;

/** `card-Activity` — what has moved money on the account, as counts, with the way to the timeline. */
export function ActivityCard({ loading, operations, truncated, error, className }: { loading: boolean; operations: Operation[]; truncated: boolean; error: string | null; className?: string }) {
  const t = useT();
  const locale = useLocale();
  const newest = operations.reduce((max, op) => Math.max(max, seconds(op.created_at)), 0);
  return (
    // The hub caps the list it answers with, so over a long history the counts are of the
    // most recent page, not of everything — the caption says which one the reader is seeing.
    <TitledCard title={t("profile.activity")} sub={truncated ? t("profile.activityCapped", { n: operations.length }) : t("profile.activitySub")} className={className}>
      {error ? (
        // A failed read is not an empty history: the box says what went wrong, not that
        // nothing has happened yet.
        <div className="py-3">
          <Empty className={EMPTY_BOX}>
            <EmptyHeader>
              <EmptyTitle>{t("profile.activity")}</EmptyTitle>
              <EmptyDescription>{error}</EmptyDescription>
            </EmptyHeader>
          </Empty>
        </div>
      ) : loading ? (
        [0, 1, 2, 3].map((i) => (
          <div key={i}>
            {i > 0 && <Hairline />}
            <Row>
              <Skeleton className="h-4 w-28" />
              <Skeleton className="h-4 w-8" />
            </Row>
          </div>
        ))
      ) : operations.length === 0 ? (
        <div className="py-3">
          <Empty className={EMPTY_BOX}>
            <EmptyHeader>
              <EmptyTitle>{t("profile.noActivityTitle")}</EmptyTitle>
              <EmptyDescription>{t("profile.noActivityBody")}</EmptyDescription>
            </EmptyHeader>
            <EmptyContent>
              {/* The action that produces the first row. Outline, like Home's — the page's
                  one filled control is not this. */}
              <Button asChild variant="outline" size="sm">
                <Link href="/wallet/deposit">{t("ui.addFunds")}</Link>
              </Button>
            </EmptyContent>
          </Empty>
        </div>
      ) : (
        <>
          {COUNTED.map((kind, i) => (
            <div key={kind}>
              {i > 0 && <Hairline />}
              <Row>
                <span className="text-sm font-medium text-ink">{kindLabel(kind, t)}</span>
                <span className="text-sm font-semibold tabular-nums text-ink">{operations.filter((op) => op.kind === kind).length}</span>
              </Row>
            </div>
          ))}
          <Hairline />
          <Row>
            <span className="text-sm font-medium text-ink">{t("profile.lastActivity")}</span>
            <RowValue className="text-ink">{dayLabel(newest, t, locale)}</RowValue>
          </Row>
          <Hairline />
          <Link href="/operations" className={cn(TEXT_LINK, "block py-3")}>
            {t("profile.viewAllActivity")}
          </Link>
        </>
      )}
    </TitledCard>
  );
}

/** `card-Verification` — the KYC state the hub actually holds, not a document checklist. */
export function VerificationCard({ loading, profile, email, className }: { loading: boolean; profile: UserProfile | null; email: string; className?: string }) {
  const t = useT();
  return (
    <ListCard className={cn("lg:px-5.5", className)}>
      <ListCardTitle sub={t("profile.identityAndStanding")}>{t("profile.identityVerification")}</ListCardTitle>
      <Hairline />
      <Row>
        <RowLabel title={t("ui.emailAddress")} sub={loading ? "…" : email || "—"} />
        {/* i18n-max: 12 — `shrink-0` Pills beside the `min-w-0` row label. */}
        {loading ? <Skeleton className="h-5 w-16 rounded-full" /> : profile?.email_verified ? <Pill icon={BadgeCheck}>{t("ui.verified")}</Pill> : <Pill tone="pending">{t("profile.unverified")}</Pill>}
      </Row>
      <Hairline />
      <Row>
        <RowLabel
          title={
            // A bare number is not a meaning: the tip says what each level lets the reader do.
            <span className="inline-flex items-center gap-1.5">
              {t("ui.kycLevel")}
              <TipAnchor anchor="profile.kyc-level" />
            </span>
          }
          sub={t("profile.kycRaisedBy")}
        />
        {loading ? <Skeleton className="h-4 w-10" /> : <RowValue className="font-semibold tabular-nums text-ink">{profile?.kyc_level ?? "—"}</RowValue>}
      </Row>
      <Hairline />
      <Row>
        <RowLabel title={t("ui.accountStatus")} sub={t("profile.platformAccess")} />
        {loading ? <Skeleton className="h-5 w-16 rounded-full" /> : profile?.status ? <Pill tone={statusTone(profile.status)}>{enumLabel("admin.status", profile.status, t)}</Pill> : <RowValue>—</RowValue>}
      </Row>
      {/* The same tier line the wallet screens gate on — see `entities/user/lib/kyc`. */}
      {isUnverified(profile) && <StartVerificationRow />}
    </ListCard>
  );
}

/** `card-Security` — how the reader signs in and where they are signed in. Preferences are
 *  the cabinet's, not the reader's, so they are not restated here. */
export function SecurityCard({ loading, email, sessions, sessionsFailed, className }: { loading: boolean; email: string; sessions: Session[] | undefined; sessionsFailed: boolean; className?: string }) {
  const t = useT();
  // A failed list is a dash, not a skeleton that never resolves.
  const devices = sessionsFailed ? "—" : sessions === undefined ? "…" : t("settings.devicesSignedIn", { n: sessions.length });
  return (
    <TitledCard title={t("ui.security")} sub={t("settings.securitySub")} className={className}>
      <Row>
        <RowLabel title={t("ui.signedInGoogle")} sub={loading ? "…" : email || "—"} />
        {/* i18n-max: 12 — a `shrink-0` Pill beside the `min-w-0` row label. */}
        <Pill>{t("settings.connected")}</Pill>
      </Row>
      <Hairline />
      <Row>
        <RowLabel title={t("ui.sessionsDevices")} sub={devices} />
        {/* i18n-max: 10 — a `shrink-0` text link beside the `min-w-0` row label. */}
        <Link href="/settings?section=sessions" className={TEXT_LINK}>
          {t("ui.manage")}
        </Link>
      </Row>
    </TitledCard>
  );
}

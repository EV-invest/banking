"use client";

// Pricing a product: the terms in force, and the form that schedules the next ones.
//
// Nothing here changes what a fund charges. Submitting SCHEDULES a change: the plane
// decides who has to agree (one administrator, or the owners' consilium when the terms
// tighten beyond the house 2 and 20) and when it binds (no earlier than 24 hours after
// scheduling while anyone holds units), and the answer says both. Every word on this
// surface has to carry that, because the failure it invites is an operator reading
// "scheduled" as "changed".

import { Loader2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle } from "@evinvest/uikit";

import { scheduleFeePolicy } from "@/entities/admin/api/admin-client";
import type { FeePolicy, FeePolicyChange } from "@/shared/contracts/admin";
import { RequestError, errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { formatMoment } from "@/shared/lib/datetime";
import { revalidateTag } from "@/shared/lib/resource";
import { isPendingChange } from "@/views/admin/fees/lib/format";
import { FIELD_LABEL_KEY, draftBps, draftProblem, draftRequirement, toRequest, type TermsDraft } from "@/views/admin/fees/lib/schedule";
import { useTermsDraft } from "@/views/admin/fees/model/use-terms-draft";
import { ScheduleFields } from "@/views/admin/fees/ui/schedule-fields";
import { TermsFields } from "@/views/admin/fees/ui/terms-fields";

export function PolicyCard({
  service,
  policy,
  onScheduled,
  focusTitle,
  onTitleFocused,
}: {
  service: string;
  policy: FeePolicy | null;
  onScheduled: (change: FeePolicyChange) => void;
  /** Take focus on the card's title — after a cancel, whose button unmounted with the
   *  pending card and would otherwise drop focus to `body`. Acknowledged through
   *  `onTitleFocused`, so a later remount does not grab focus again. */
  focusTitle: boolean;
  onTitleFocused: () => void;
}) {
  const t = useT();
  const locale = useLocale();
  const current = policy?.configured ? policy : null;
  const { draft, set, reset } = useTermsDraft(current);
  const [busy, setBusy] = useState(false);
  // `busy` disables the button, but a state update lands a frame later than the second
  // click of a double-click does; the ref is read in the same tick and closes the gap.
  const inFlight = useRef(false);
  const [problem, setProblem] = useState<string | null>(null);
  // The plane's refusal is about the draft as it was sent; the first edit makes it stale,
  // and a red sentence that outlives the mistake it named reads as a second mistake.
  const edit = useCallback(
    <K extends keyof TermsDraft>(field: K, value: TermsDraft[K]) => {
      setProblem(null);
      set(field, value);
    },
    [set],
  );
  // The reason becomes required the moment a rate crosses the envelope, which is before
  // the operator has been anywhere near the field — so "required" is said as a label at
  // once and as an ERROR only after they have touched it.
  const [reasonTouched, setReasonTouched] = useState(false);
  // Read once per card: the horizon and the picker's bounds are measured from it, and a
  // clock that ticked on every keystroke would move them under the operator. A card lives
  // for one fund's one change, so it is never stale by more than that.
  const [now] = useState(() => Math.floor(Date.now() / 1000));
  const titleRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!focusTitle) return;
    titleRef.current?.focus();
    onTitleFocused();
  }, [focusTitle, onTitleFocused]);

  // One change per product at a time: the plane refuses a second until the first is
  // cancelled, so the form says so instead of offering a click that will be refused.
  const blocked = isPendingChange(policy?.pending?.state);

  // One conversion, read by the validator, the showcase and the request alike, so the figure
  // the operator is shown before scheduling is the same integer the request carries.
  const bps = useMemo(() => draftBps(draft), [draft]);
  const requirement = useMemo(() => draftRequirement(current, draft), [current, draft]);
  const found = useMemo(() => draftProblem(current, draft, now), [current, draft, now]);
  const reasonMissing = found?.key === "admin.fees.err.reasonRequired";
  // Too long is said at once, unlike "required": it is about text they have typed.
  const reasonTooLong = found?.key === "admin.fees.err.reasonTooLong" ? t(found.key, { max: found.max, used: found.used }) : null;
  const tooFarAhead = found?.key === "admin.fees.err.tooFarAhead" ? t(found.key, { days: found.days }) : null;
  // The rate problems, in words. The field name is interpolated rather than concatenated
  // onto the front: which end of the sentence it belongs at is a per-language decision.
  const rateProblem = useMemo(() => {
    if (found?.key === "admin.fees.err.overCeiling") return t(found.key, { field: t(FIELD_LABEL_KEY[found.field]), ceiling: found.ceiling });
    if (found?.key === "admin.fees.err.notPercent") return t(found.key, { field: t(FIELD_LABEL_KEY[found.field]) });
    return null;
  }, [found, t]);
  const invalid = found !== null;

  async function schedule() {
    if (invalid || blocked || inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setProblem(null);
    try {
      const change = await scheduleFeePolicy(toRequest(service, draft));
      // The policy list now carries the change as `pending`, and the history grew a row.
      // The receipt is the parent's: the re-read remounts this card on the new pending
      // change, and a receipt held here would vanish with the old instance.
      revalidateTag(TAG.adminFees);
      onScheduled(change);
      reset();
    } catch (e) {
      setProblem(e instanceof Error ? errorMessage(e, t) : t("err.feePolicySave"));
      // "Already pending" means a colleague got there first, and this screen still shows
      // the fund without their change. The re-read brings their pending card in and
      // blocks this form for the right reason, instead of leaving a red sentence about a
      // card the operator cannot see.
      if (e instanceof RequestError && e.status === 409) revalidateTag(TAG.adminFees);
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }

  return (
    <Card className="h-fit gap-4">
      <CardHeader className="gap-1">
        {/* A heading, not a bare title: focus lands here after a cancel, and a screen
            reader should say what it landed on. `CardTitle` renders a div. */}
        <CardTitle ref={titleRef} tabIndex={-1} role="heading" aria-level={2} className="rounded-sm text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring">
          {t("admin.fees.terms")}
        </CardTitle>
        <CardDescription className="text-xs text-ink-soft">
          {current ? t("admin.fees.inForce", { version: current.version, since: formatMoment(current.effective_from, locale) }) : t("admin.fees.notConfigured")}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <TermsFields draft={draft} bps={bps} onChange={edit} disabled={blocked} />
        <ScheduleFields
          draft={draft}
          now={now}
          requirement={requirement}
          effectiveFromError={tooFarAhead}
          reasonError={reasonMissing && reasonTouched ? t("admin.fees.err.reasonRequired") : reasonTooLong}
          onChange={edit}
          onReasonTouched={() => setReasonTouched(true)}
          disabled={blocked}
        />

        {blocked && <p className="text-xs text-ink-soft">{t("admin.fees.pendingBlocks")}</p>}
        {rateProblem && !blocked && <p className="text-xs text-destructive">{rateProblem}</p>}
        {problem && <p className="text-xs text-destructive">{problem}</p>}

        <Button type="button" onClick={schedule} disabled={busy || blocked || invalid}>
          {busy && <Loader2 className="size-4 animate-spin" />}
          {requirement === "owner_consilium" ? t("admin.fees.askOwners") : current ? t("admin.fees.scheduleChange") : t("admin.fees.startCharging")}
        </Button>
      </CardContent>
    </Card>
  );
}

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
import { useEffect, useMemo, useRef, useState } from "react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Button, Card, CardContent } from "@evinvest/uikit";

import { scheduleFeePolicy } from "@/entities/admin/api/admin-client";
import type { FeePolicy, FeePolicyChange } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { formatMoment } from "@/shared/lib/datetime";
import { revalidateTag } from "@/shared/lib/resource";
import { isPendingChange } from "@/views/admin/fees/lib/format";
import { FIELD_LABEL_KEY, draftBps, draftProblem, draftRequirement, toRequest } from "@/views/admin/fees/lib/schedule";
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
  const [problem, setProblem] = useState<string | null>(null);
  // The reason becomes required the moment a rate crosses the envelope, which is before
  // the operator has been anywhere near the field — so "required" is said as a label at
  // once and as an ERROR only after they have touched it.
  const [reasonTouched, setReasonTouched] = useState(false);
  const titleRef = useRef<HTMLParagraphElement>(null);
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
  const found = useMemo(() => draftProblem(current, draft), [current, draft]);
  const reasonMissing = found?.key === "admin.fees.err.reasonRequired";
  // The rate problems, in words. The field name is interpolated rather than concatenated
  // onto the front: which end of the sentence it belongs at is a per-language decision.
  const rateProblem = useMemo(() => {
    if (!found || found.key === "admin.fees.err.reasonRequired") return null;
    if (found.key === "admin.fees.err.overCeiling") return t(found.key, { field: t(FIELD_LABEL_KEY[found.field]), ceiling: found.ceiling });
    return t(found.key, { field: t(FIELD_LABEL_KEY[found.field]) });
  }, [found, t]);
  const invalid = found !== null;

  async function schedule() {
    if (invalid || blocked) return;
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
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card className="h-fit">
      <CardContent className="space-y-4 py-6">
        <div className="space-y-1">
          <p ref={titleRef} tabIndex={-1} className="rounded-sm text-sm font-semibold outline-none focus-visible:ring-2 focus-visible:ring-ring">
            {t("admin.fees.terms")}
          </p>
          <p className="text-xs text-muted-foreground">
            {current ? t("admin.fees.inForce", { version: current.version, since: formatMoment(current.effective_from, locale) }) : t("admin.fees.notConfigured")}
          </p>
        </div>

        <TermsFields draft={draft} bps={bps} onChange={set} disabled={blocked} />
        <ScheduleFields
          draft={draft}
          requirement={requirement}
          reasonError={reasonMissing && reasonTouched ? t("admin.fees.err.reasonRequired") : null}
          onChange={set}
          onReasonTouched={() => setReasonTouched(true)}
          disabled={blocked}
        />

        {blocked && <p className="text-xs text-muted-foreground">{t("admin.fees.pendingBlocks")}</p>}
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

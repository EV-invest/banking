"use client";

// The holders a scheduled change could not reach, and the one act that binds it over them.
//
// Taking responsibility is not a retry of the mailer: the terms bind over holders who were
// never told, and the operator's name goes on the change for good. So the button opens a
// second step in place, the same way cancelling does (`./pending-card.tsx`) — no
// `window.confirm`, and focus lands on the safe answer.

import { Loader2, MailWarning } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, AlertTitle, Button } from "@evinvest/uikit";

import { acknowledgeUndeliveredNotices } from "@/entities/admin/api/admin-client";
import type { FeePolicyChange } from "@/shared/contracts/admin";
import { errorMessage, RequestError } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { formatMoment } from "@/shared/lib/datetime";
import { revalidateTag } from "@/shared/lib/resource";
import { noticeSummary } from "@/views/admin/fees/lib/notices";

export function NoticeWaiver({ change }: { change: FeePolicyChange }) {
  const t = useT();
  const locale = useLocale();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const keepRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (confirming) keepRef.current?.focus();
  }, [confirming]);

  const summary = noticeSummary(change);
  if (summary.kind === "none") return null;

  if (summary.kind === "waived") {
    return (
      <p className="text-xs text-muted-foreground">
        {t("admin.fees.waiver.acknowledged", { by: summary.by || "—", at: formatMoment(summary.at, locale), n: summary.holders })}
      </p>
    );
  }

  async function acknowledge() {
    setBusy(true);
    setProblem(null);
    try {
      await acknowledgeUndeliveredNotices({ service: change.service, change_id: change.id });
      // The pending change now carries the waiver and the history row shows it — both
      // readers of the tag. A 409 means the figures moved under us (the notices arrived,
      // or the change is no longer scheduled), so the same reread is the right answer.
      revalidateTag(TAG.adminFees);
      setConfirming(false);
    } catch (e) {
      if (e instanceof RequestError && e.status === 409) revalidateTag(TAG.adminFees);
      setProblem(waiverProblem(e, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Alert role="status" className="border-main-accent-t3/40 bg-main-accent-t3/10">
      <MailWarning className="size-4 text-main-accent-t3" />
      <AlertTitle>{t("admin.fees.notices.undelivered", { n: summary.undelivered })}</AlertTitle>
      <AlertDescription className="gap-3 text-foreground">
        <p className="text-sm leading-relaxed">
          {t("admin.fees.notices.waits")}
          {summary.givenUp > 0 && <> {t("admin.fees.notices.givenUp", { n: summary.givenUp })}</>}
        </p>
        {problem && <p className="text-xs text-destructive">{problem}</p>}
        {confirming ? (
          <>
            <p className="text-sm leading-relaxed">{t("admin.fees.notices.acknowledgeWarning", { n: summary.undelivered })}</p>
            <div className="flex flex-col gap-2.5 sm:flex-row">
              <Button variant="destructive" size="sm" disabled={busy} onClick={() => void acknowledge()}>
                {busy && <Loader2 className="size-4 animate-spin" />}
                {t("admin.fees.notices.acknowledgeConfirm")}
              </Button>
              <Button ref={keepRef} variant="ghost" size="sm" disabled={busy} onClick={() => setConfirming(false)}>
                {t("admin.fees.notices.acknowledgeBack")}
              </Button>
            </div>
          </>
        ) : (
          <div>
            <Button variant="outline" size="sm" onClick={() => setConfirming(true)}>
              {t("admin.fees.notices.acknowledge")}
            </Button>
          </div>
        )}
      </AlertDescription>
    </Alert>
  );
}

// The two refusals the hub raises on purpose, in words that say what happened; anything
// else is a fault and reads as one.
function waiverProblem(e: unknown, t: (key: string) => string): string {
  if (e instanceof RequestError && e.status === 403) return t("admin.fees.notices.err.forbidden");
  if (e instanceof RequestError && e.status === 409) return t("admin.fees.notices.err.nothing");
  return errorMessage(e, t);
}

/** The waiver as one line of history — who, and over how many — under the row's state. */
export function WaiverNote({ change }: { change: FeePolicyChange }) {
  const t = useT();
  const summary = noticeSummary(change);
  if (summary.kind !== "waived") return null;
  return <p className="mt-1 text-xs text-muted-foreground">{t("admin.fees.waiver.row", { by: summary.by || "—", n: summary.holders })}</p>;
}

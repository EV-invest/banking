"use client";

// The holders a scheduled change could not reach, and the one act that binds it over them.
//
// Taking responsibility is not a retry of the mailer: the terms bind over holders who were
// never told, and the operator's name goes on the change for good. So the button opens a
// second step in place, the same way cancelling does (`./pending-card.tsx`) — no
// `window.confirm`, and focus lands on the safe answer.
//
// The act is offered only over notices the mailer has GIVEN UP on — the hub refuses it
// otherwise — so a notice still in the queue is reported as exactly that, with nothing to
// press: the half-minute after scheduling, when every notice is undelivered because none
// has been tried yet, must not read as holders who could not be told. And it is offered
// again when the mailer gives up on more holders after an acknowledgement: the record
// covers exactly the holders it was given over, so the later ones need an act of their
// own — one that extends the list and puts the extender's name on all of it.

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
import { noticeSummary, type Waiver, waiverRecord } from "@/views/admin/fees/lib/notices";

export function NoticeWaiver({ change }: { change: FeePolicyChange }) {
  const t = useT();
  const locale = useLocale();
  const summary = noticeSummary(change);
  switch (summary.kind) {
    case "none":
      return null;
    case "queued":
      return <p className="text-xs text-ink-soft">{t("admin.fees.notices.queued", { n: summary.queued })}</p>;
    case "waived": {
      const at = formatMoment(summary.at, locale);
      return (
        <p className="text-xs text-ink-soft">
          {summary.by
            ? t("admin.fees.waiver.acknowledged", { by: summary.by, at, n: summary.holders })
            : t("admin.fees.waiver.acknowledgedAnon", { at, n: summary.holders })}
        </p>
      );
    }
    case "givenUp":
      return <GivenUpNotices change={change} givenUp={summary.givenUp} queued={summary.queued} waiver={summary.waiver} />;
  }
}

function GivenUpNotices({ change, givenUp, queued, waiver }: { change: FeePolicyChange; givenUp: number; queued: number; waiver: Waiver | null }) {
  const t = useT();
  const locale = useLocale();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  // Focus follows the step both ways: in, onto the safe answer (see `./pending-card.tsx`);
  // out by "Keep waiting", back onto the button that opened it — which is remounted, so
  // the browser would otherwise drop focus to `body`.
  const keepRef = useRef<HTMLButtonElement>(null);
  const openRef = useRef<HTMLButtonElement>(null);
  const wasConfirming = useRef(false);
  useEffect(() => {
    if (confirming) keepRef.current?.focus();
    else if (wasConfirming.current) openRef.current?.focus();
    wasConfirming.current = confirming;
  }, [confirming]);

  async function acknowledge() {
    setBusy(true);
    setProblem(null);
    try {
      await acknowledgeUndeliveredNotices({ service: change.service, change_id: change.id });
      // The pending change now carries the waiver and the history row shows it — both
      // readers of the tag.
      revalidateTag(TAG.adminFees);
      setConfirming(false);
    } catch (e) {
      // A 409 means the figures moved under us — the notices arrived, the mailer gave up
      // on none after all, the change is no longer scheduled — or the change only loosens
      // the terms and needs no act. The hub says which in a sentence of its own; the same
      // reread that answers a success brings in the figures it refused on.
      if (e instanceof RequestError && e.status === 409) revalidateTag(TAG.adminFees);
      setProblem(waiverProblem(e, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    // The tint sits on the root, not the icon: the uikit `Alert` paints its icon
    // `currentColor` with a selector that outranks a colour class on the `svg` itself.
    <Alert role="status" className="border-accent-warn/40 bg-accent-warn/10 text-accent-warn">
      <MailWarning className="size-4" />
      <AlertTitle className="text-ink">{t("admin.fees.notices.givenUpTitle", { n: givenUp })}</AlertTitle>
      <AlertDescription className="gap-3 text-ink">
        <p className="text-sm leading-relaxed">
          {t("admin.fees.notices.givenUpBody", { n: givenUp })}
          {queued > 0 && <> {t("admin.fees.notices.moreQueued", { n: queued })}</>}
        </p>
        {waiver && (
          <p className="text-sm leading-relaxed">
            {waiver.by
              ? t("admin.fees.notices.alreadyWaived", { by: waiver.by, at: formatMoment(waiver.at, locale), n: waiver.holders })
              : t("admin.fees.notices.alreadyWaivedAnon", { at: formatMoment(waiver.at, locale), n: waiver.holders })}
          </p>
        )}
        {problem && <p className="text-xs text-accent-error">{problem}</p>}
        {confirming ? (
          <>
            <p className="text-sm leading-relaxed">
              {waiver
                ? t("admin.fees.notices.acknowledgeWarningExtend", { n: givenUp, total: waiver.holders + givenUp })
                : t("admin.fees.notices.acknowledgeWarning", { n: givenUp })}
            </p>
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
          <div className="flex flex-wrap gap-2">
            <Button ref={openRef} variant="outline" size="sm" onClick={() => setConfirming(true)}>
              {t("admin.fees.notices.acknowledge")}
            </Button>
          </div>
        )}
      </AlertDescription>
    </Alert>
  );
}

// The hub's refusals in its own words: a 409 names which figure moved (`errorMessage`
// relays hub prose verbatim, see `api-client.ts`), and only a bare 403 — not the stale
// page's `csrf`, which carries a key of its own — is the "not yours to acknowledge" one.
function waiverProblem(e: unknown, t: (key: string) => string): string {
  if (e instanceof RequestError && e.status === 403 && !e.code) return t("admin.fees.notices.err.forbidden");
  return errorMessage(e, t);
}

/** The waiver as one line of history — who, and over how many — under the row's state.
 *  Read off the record, not the summary: it stays history while more are given up on. */
export function WaiverNote({ change }: { change: FeePolicyChange }) {
  const t = useT();
  const waiver = waiverRecord(change);
  if (!waiver) return null;
  return (
    <p className="mt-1 text-xs text-ink-soft">
      {waiver.by ? t("admin.fees.waiver.row", { by: waiver.by, n: waiver.holders }) : t("admin.fees.waiver.rowAnon", { n: waiver.holders })}
    </p>
  );
}

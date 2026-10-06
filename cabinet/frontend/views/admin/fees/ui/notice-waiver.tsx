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

import { MailWarning } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import type { Translate } from "@evinvest/i18n";
import { useLocale, useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, AlertTitle, Button, Spinner } from "@evinvest/uikit";

import { acknowledgeUndeliveredNotices } from "@/entities/admin/api/admin-client";
import type { FeePolicyChange } from "@/shared/contracts/admin";
import { errorMessage, RequestError } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { formatMoment } from "@/shared/lib/datetime";
import { stripTransportPrefix } from "@/shared/lib/hub-refusal";
import { revalidateTag } from "@/shared/lib/resource";
import { noticeSummary, type Waiver, waiverName, waiverRecord } from "@/views/admin/fees/lib/notices";

// The sentence names the operator by email; the id stays one hover away as the record's
// own fact. Nothing to hover when the id is what the sentence already says.
const idTooltip = (waiver: Pick<Waiver, "by" | "email">): string | undefined => (waiver.email?.trim() && waiver.by ? waiver.by : undefined);

export function NoticeWaiver({ change }: { change: FeePolicyChange }) {
  const t = useT();
  const locale = useLocale();
  const summary = noticeSummary(change);
  switch (summary.kind) {
    case "none":
      return null;
    case "queued":
      return <p className="text-xs text-ink-soft">{t("admin.fees.notices.queued", "{n, plural, one {# notice is} other {# notices are}} still being delivered. A change that tightens the terms waits for every holder to be told; there is nothing to do unless the mailer gives up on one.", { n: summary.queued })}</p>;
    case "waived": {
      const at = formatMoment(summary.at, locale);
      const by = waiverName(summary);
      return (
        <p className="text-xs text-ink-soft" title={idTooltip(summary)}>
          {by ? t("admin.fees.waiver.acknowledged", "Notices waived by {by} on {at}, over {n, plural, one {# holder} other {# holders}} who could not be told.", { by, at, n: summary.holders }) : t("admin.fees.waiver.acknowledgedAnon", "Notices waived on {at}, over {n, plural, one {# holder} other {# holders}} who could not be told.", { at, n: summary.holders })}
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
  const waiverBy = waiver ? waiverName(waiver) : null;
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
      <AlertTitle className="text-ink">{t("admin.fees.notices.givenUpTitle", "{n, plural, one {# holder} other {# holders}} could not be told", { n: givenUp })}</AlertTitle>
      <AlertDescription className="gap-3 text-ink">
        <p className="text-sm leading-relaxed">
          {t("admin.fees.notices.givenUpBody", "The mailer has given up on {n, plural, one {this holder's notice} other {these # notices}}. A change that tightens the terms does not bind over a holder who was never told, so it is held up until somebody takes responsibility for them.", { n: givenUp })}
          {queued > 0 && <> {t("admin.fees.notices.moreQueued", "{n, plural, one {# more notice is still being delivered and is not covered by this act.} other {# more notices are still being delivered and are not covered by this act.}}", { n: queued })}</>}
        </p>
        {waiver && (
          <p className="text-sm leading-relaxed" title={idTooltip(waiver)}>
            {waiverBy
              ? t("admin.fees.notices.alreadyWaived", "{by} already took responsibility on {at} for {n, plural, one {# holder} other {# holders}} the mailer had given up on by then. It has given up on more since; this act adds them to that record.", { by: waiverBy, at: formatMoment(waiver.at, locale), n: waiver.holders })
              : t("admin.fees.notices.alreadyWaivedAnon", "Responsibility was already taken on {at} for {n, plural, one {# holder} other {# holders}} the mailer had given up on by then. It has given up on more since; this act adds them to that record.", { at: formatMoment(waiver.at, locale), n: waiver.holders })}
          </p>
        )}
        {problem && <p className="text-xs text-accent-error">{problem}</p>}
        {confirming ? (
          <>
            <p className="text-sm leading-relaxed">
              {waiver
                ? t("admin.fees.notices.acknowledgeWarningExtend", "The terms will bind over {n, plural, one {this holder} other {these # holders}} without their notice — tighter terms they were never told about. Your name and the moment replace the earlier acknowledgement's on the change, which will then cover {total, plural, one {# holder} other {all # holders}}, and stay in its history.", { n: givenUp, total: waiver.holders + givenUp })
                : t("admin.fees.notices.acknowledgeWarning", "The terms will bind over {n, plural, one {this holder} other {these # holders}} without their notice — tighter terms they were never told about. Your name, the moment, and exactly which holders it covers are recorded on the change and stay in its history.", { n: givenUp })}
            </p>
            <div className="flex flex-col gap-2.5 sm:flex-row">
              <Button variant="destructive" size="sm" disabled={busy} onClick={() => void acknowledge()}>
                {busy && <Spinner aria-hidden />}
                {t("admin.fees.notices.acknowledgeConfirm", "Take responsibility")}
              </Button>
              <Button ref={keepRef} variant="ghost" size="sm" disabled={busy} onClick={() => setConfirming(false)}>
                {t("admin.fees.notices.acknowledgeBack", "Keep waiting")}
              </Button>
            </div>
          </>
        ) : (
          <div className="flex flex-wrap gap-2">
            <Button ref={openRef} variant="outline" size="sm" onClick={() => setConfirming(true)}>
              {t("admin.fees.notices.acknowledge", "Take responsibility and bind anyway")}
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
function waiverProblem(e: unknown, t: Translate): string {
  if (e instanceof RequestError && e.status === 403 && !e.code) return t("admin.fees.notices.err.forbidden", "Only the requester of this change or an owner can take responsibility for it.");
  return stripTransportPrefix(errorMessage(e, t));
}

/** The waiver as one line of history — who, and over how many — under the row's state.
 *  Read off the record, not the summary: it stays history while more are given up on. */
export function WaiverNote({ change }: { change: FeePolicyChange }) {
  const t = useT();
  const waiver = waiverRecord(change);
  if (!waiver) return null;
  const by = waiverName(waiver);
  return (
    <p className="mt-1 text-xs text-ink-soft" title={idTooltip(waiver)}>
      {by ? t("admin.fees.waiver.row", "Notices waived by {by} · {n, plural, one {# holder} other {# holders}}", { by, n: waiver.holders }) : t("admin.fees.waiver.rowAnon", "Notices waived · {n, plural, one {# holder} other {# holders}}", { n: waiver.holders })}
    </p>
  );
}

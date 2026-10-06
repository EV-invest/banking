"use client";

// "Do you approve this payout?" — reached from an email, with no session and no way back.
//
// The shape of this screen is decided by two facts from docs/CONSILIUM.md that have nothing
// to do with visual design:
//
//   · The GET is inert (policy 5). Mail scanners issue an automatic GET on every link in a
//     message, so arriving here must change nothing. Nothing on this page fires on mount
//     except the read that renders it, and the read spends no token.
//   · The reader approves what they can see (policy 12–13). The full destination address,
//     the exact amount, the network, the memo and the fingerprint are all on screen before
//     the code field is — because `payload_hash` is re-verified at execution against
//     exactly these values, and an owner who was shown a truncated address approved
//     something else.
//
// A consilium carries one of several things — a payment order (`invitation.payment`,
// docs/CONSILIUM.md § Payments), a NAV mark past the move guard (`invitation.valuation_override`, banking#232), a change of a product's fee
// terms (`invitation.fee_policy`, docs/FEES.md § Changing the terms), a person seated on
// a reserved allocation (`invitation.holder_grant`) or a seed of the platform's capital
// (`invitation.seed_capital`, both #245). The page is the same in every case; only the
// terms card and the words naming what is approved change.
//
// The tally, the attempt counter and the settled decision are all read back from the
// server. This page never decrements, increments or predicts any of them: a wrong code is
// counted in the same transaction as the comparison (policy 7), so a number this component
// worked out for itself would at best duplicate the server's and at worst contradict it.

import { CheckCircle2, XCircle } from "lucide-react";
import { useState } from "react";

import type { Translate } from "@evinvest/i18n";
import { useLocale, useT } from "@evinvest/i18n/react";
import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle, Progress, Separator, Spinner } from "@evinvest/uikit";

import { ApprovalUnavailableError } from "@/entities/approval/api/approval-client";
import { decidePayout, payoutApprovalResource } from "@/entities/approval/model/approval-resource";
import type { PayoutDecision } from "@/shared/contracts/governance";
import { errorMessage } from "@/shared/lib/api-client";
import { expiresIn, formatMoment, hasExpired } from "@/shared/lib/datetime";
import { settledPayout } from "@/shared/lib/decision";
import { stripTransportPrefix, wrongCodeAttempts } from "@/shared/lib/hub-refusal";
import { useResource } from "@/shared/lib/resource";
import { ResourceError } from "@/shared/ui/resource-error";
import {
  ApprovalBurned,
  ApprovalExpired,
  ApprovalOutcome,
  ApprovalPage,
  ApprovalSkeleton,
  ApprovalTitle,
  ApprovalUnavailable,
  ApprovalUnreachable,
  ApprovalUnrenderable,
  CodeField,
  DetailRow,
} from "@/views/approval/ui/approval-chrome";
import { FeePolicyTermsBlock, renderableFeePolicy } from "@/views/approval/ui/fee-policy-terms";
import { HolderGrantTermsBlock, SeedCapitalTermsBlock, renderableHolderGrant, renderableSeedCapital } from "@/views/approval/ui/ownership-terms";
import { PaymentTermsBlock, renderablePayment } from "@/views/approval/ui/payment-terms";
import { ValuationTermsBlock, renderableValuation } from "@/views/approval/ui/valuation-terms";

type ApprovalSubject = "valuation" | "payment" | "feePolicy" | "holderGrant" | "seedCapital";

interface ApprovalWords {
  title: string;
  lead: (initiator: string) => string;
  approve: string;
  approvedTitle: string;
  rejectedTitle: string;
  freshBody: string;
}

// One family of words per kind of thing being approved; the rest of the page is shared.
const WORDS: Record<ApprovalSubject, (t: Translate) => ApprovalWords> = {
  valuation: (t) => ({
    title: t("approval.valuation.title", "Approve a NAV mark"),
    lead: (initiator) => t("approval.valuation.lead", "{initiator} has asked to mark a fund at a value the NAV-move guard refuses. A mark past the guard is never one person's decision, so it needs the owners to agree.", { initiator }),
    approve: t("approval.valuation.approve", "Approve this mark"),
    approvedTitle: t("approval.valuation.decided.approvedTitle", "You approved this mark"),
    rejectedTitle: t("approval.valuation.decided.rejectedTitle", "You rejected this mark"),
    freshBody: t("approval.valuation.decided.freshBody", "Your answer has been recorded. There is nothing further to do here — the fund is marked once enough owners have agreed."),
  }),
  payment: (t) => ({
    title: t("approval.payment.title", "Approve a fund payment"),
    lead: (initiator) => t("approval.payment.lead", "{initiator} has asked to move money the fund owns, as set out below. Moving the fund's own money is never one person's decision, so it needs the owners to agree.", { initiator }),
    approve: t("approval.payment.approve", "Approve this payment"),
    approvedTitle: t("approval.payment.decided.approvedTitle", "You approved this payment"),
    rejectedTitle: t("approval.payment.decided.rejectedTitle", "You rejected this payment"),
    freshBody: t("approval.payment.decided.freshBody", "Your answer has been recorded. There is nothing further to do here — the payment goes ahead once enough owners have agreed."),
  }),
  feePolicy: (t) => ({
    title: t("approval.feePolicy.title", "Approve a change of fee terms"),
    lead: (initiator) => t("approval.feePolicy.lead", "{initiator} has asked to change what a product charges its investors, as set out below. Tightening the terms beyond what the prospectus promised is never one person's decision, so it needs the owners to agree.", { initiator }),
    approve: t("approval.feePolicy.approve", "Approve this change"),
    approvedTitle: t("approval.feePolicy.decided.approvedTitle", "You approved this change"),
    rejectedTitle: t("approval.feePolicy.decided.rejectedTitle", "You rejected this change"),
    freshBody: t("approval.feePolicy.decided.freshBody", "Your answer has been recorded. There is nothing further to do here — the change is scheduled once enough owners have agreed, and binds 24 hours after that while anyone holds units."),
  }),
  holderGrant: (t) => ({
    title: t("approval.holderGrant.title", "Approve a holder grant"),
    lead: (initiator) => t("approval.holderGrant.lead", "{initiator} has asked to seat a person on one of the platform's own allocations, as set out below. Who holds the platform's money is never one person's decision, so it needs the owners to agree.", { initiator }),
    approve: t("approval.holderGrant.approve", "Approve this grant"),
    approvedTitle: t("approval.holderGrant.decided.approvedTitle", "You approved this grant"),
    rejectedTitle: t("approval.holderGrant.decided.rejectedTitle", "You rejected this grant"),
    freshBody: t("approval.holderGrant.decided.freshBody", "Your answer has been recorded. There is nothing further to do here — the units are minted once enough owners have agreed."),
  }),
  seedCapital: (t) => ({
    title: t("approval.seedCapital.title", "Approve seed capital"),
    lead: (initiator) => t("approval.seedCapital.lead", "{initiator} has asked to attribute a transfer to the treasury as a person's seed of the platform's capital, as set out below. Whose capital it is is never one person's decision, so it needs the owners to agree.", { initiator }),
    approve: t("approval.seedCapital.approve", "Approve this seed"),
    approvedTitle: t("approval.seedCapital.decided.approvedTitle", "You approved this seed"),
    rejectedTitle: t("approval.seedCapital.decided.rejectedTitle", "You rejected this seed"),
    freshBody: t("approval.seedCapital.decided.freshBody", "Your answer has been recorded. There is nothing further to do here — the deposit is booked once enough owners have agreed."),
  }),
};

export function PayoutApprovalView({ token }: { token: string }) {
  const t = useT();
  const locale = useLocale();

  // The read starts from the resource's own subscription, not from an effect here.
  const summary = useResource(payoutApprovalResource, token);
  const invitation = summary.data ?? null;

  const [code, setCode] = useState("");
  const [pending, setPending] = useState<PayoutDecision | null>(null);
  const [actionError, setActionError] = useState<unknown>(null);
  // Set only from a server figure that actually moved — see `decide`.
  const [rejectedAttempts, setRejectedAttempts] = useState<number | null>(null);
  const [justDecided, setJustDecided] = useState(false);
  /** A vote met the single 404: the token is spent, burned or gone. Which one is not ours to know. */
  const [spent, setSpent] = useState(false);

  // NOT `?? null`: an unanswered seat arrives as the string "pending" (see
  // `shared/lib/decision.ts`), which is truthy and would mark every fresh invitation
  // settled — telling an owner they rejected a payout they have never seen.
  const settled = settledPayout(invitation?.decision);
  // Once this seat has answered, the outcome stands and later failures are not news: a
  // background revalidation 404s the moment the token is spent, and flipping a recorded
  // "you approved this" into "this link is dead" would be alarming and wrong.
  const gone = spent || (!settled && summary.error instanceof ApprovalUnavailableError);

  const decide = async (decision: PayoutDecision) => {
    const secret = code.trim().toUpperCase();
    if (!secret || pending) return;
    const attemptsBefore = invitation?.attempts_remaining ?? null;
    setPending(decision);
    setActionError(null);
    // Cleared per attempt, not per success. Left standing it would keep showing a count
    // from the previous try AND suppress the error banner for this one, so a network
    // failure after a wrong code would report nothing at all.
    setRejectedAttempts(null);
    try {
      const result = await decidePayout(token, secret, decision);
      setCode("");
      if (result.decided) {
        setJustDecided(true);
        setRejectedAttempts(null);
      } else {
        setRejectedAttempts(result.invitation.attempts_remaining);
      }
    } catch (cause) {
      if (cause instanceof ApprovalUnavailableError) {
        // The token was spent or burned by this very attempt. Which one is not knowable
        // from here, and the page does not guess (policy 10).
        setSpent(true);
        return;
      }
      setActionError(cause);
      setCode("");
      // A refused code may come back as an error rather than as a 200 that did not decide:
      // "validation failed: incorrect code — 4 attempts remaining", the plane's own count
      // behind the BFF's prefix (banking#324). The figure is the server's, so it goes under
      // the field at once instead of reaching the owner as that sentence.
      const attemptsLeft = wrongCodeAttempts(cause);
      if (attemptsLeft !== null) setRejectedAttempts(attemptsLeft);
      // Either way the authority on how many attempts are left is the server, so ask it
      // rather than assuming this failure consumed one — a network error did not. Read
      // through `settledPayout`: an open seat arrives as the truthy "pending", and a bare
      // `!decision` never fired here, which is how the sentence above reached the screen.
      await summary.refresh();
      const fresh = payoutApprovalResource.peek(token);
      if (fresh && attemptsBefore !== null && fresh.attempts_remaining < attemptsBefore && !settledPayout(fresh.decision)) {
        setRejectedAttempts(fresh.attempts_remaining);
      }
    } finally {
      setPending(null);
    }
  };

  if (gone) {
    return (
      <ApprovalPage>
        <ApprovalUnavailable />
      </ApprovalPage>
    );
  }

  if (!invitation) {
    return (
      <ApprovalPage>
        {summary.isLoading ? (
          <ApprovalSkeleton />
        ) : (
          <ApprovalUnreachable onRetry={() => void summary.refresh()} retrying={summary.isValidating} />
        )}
      </ApprovalPage>
    );
  }

  const payment = invitation.payment ?? null;
  const valuation = invitation.valuation_override ?? null;
  const feePolicy = invitation.fee_policy ?? null;
  const grant = invitation.holder_grant ?? null;
  const seed = invitation.seed_capital ?? null;
  // The terms an owner is agreeing to must actually be on screen. An empty string is not
  // nullish, so a `?? "-"` renders nothing at all while the Approve button stays live. That
  // is precisely the approval-of-something-unseen policy 12/13 exists to prevent, so a
  // request whose terms did not arrive — or whose kind this page does not know — is not
  // offered for decision at all.
  const renderable = valuation
    ? renderableValuation(valuation)
    : payment
      ? renderablePayment(payment)
      : feePolicy
        ? renderableFeePolicy(feePolicy)
        : grant
          ? renderableHolderGrant(grant)
          : seed !== null && renderableSeedCapital(seed);
  // Past the `renderable` guard below, the last arm can only be a seed.
  const words = WORDS[valuation ? "valuation" : payment ? "payment" : feePolicy ? "feePolicy" : grant ? "holderGrant" : "seedCapital"](t);
  const burned = !settled && (invitation.attempts_remaining ?? 0) <= 0;
  const expired = !settled && hasExpired(invitation.expires_at);
  const threshold = invitation.threshold ?? 0;
  const approvals = invitation.approvals ?? 0;
  // A zero threshold is nonsense the server should never send; a full bar for it would read
  // as "everyone has approved", so it reads as nothing instead.
  const progress = threshold > 0 ? Math.min(100, Math.round((approvals / threshold) * 100)) : 0;

  if (!renderable) {
    return (
      <ApprovalPage>
        <ApprovalUnrenderable onRetry={() => void summary.refresh()} retrying={summary.isValidating} />
      </ApprovalPage>
    );
  }

  return (
    <ApprovalPage>
      <Card>
        <CardHeader>
          <ApprovalTitle>{words.title}</ApprovalTitle>
          <CardDescription className="text-balance">
            {words.lead(invitation.initiator_email)}
          </CardDescription>
        </CardHeader>

        <CardContent className="flex flex-col gap-5">
          {valuation ? (
            <ValuationTermsBlock terms={valuation} payloadHash={invitation.payload_hash} />
          ) : payment ? (
            <PaymentTermsBlock terms={payment} payloadHash={invitation.payload_hash} reasonLabel={t("approval.payment.reasonLabel", "Reason given")} />
          ) : feePolicy ? (
            <FeePolicyTermsBlock terms={feePolicy} payloadHash={invitation.payload_hash} />
          ) : grant ? (
            <HolderGrantTermsBlock terms={grant} payloadHash={invitation.payload_hash} />
          ) : (
            seed && <SeedCapitalTermsBlock terms={seed} payloadHash={invitation.payload_hash} />
          )}

          <div className="flex flex-col gap-2.5">
            <DetailRow label={t("approval.openedBy", "Opened by")} value={invitation.initiator_email} />
            <DetailRow label={t("approval.openedAt", "Opened")} value={formatMoment(invitation.created_at, locale)} />
            <DetailRow
              label={t("approval.expires", "Expires")}
              value={t("approval.expiresValue", "{at} · {left}", { at: formatMoment(invitation.expires_at, locale), left: expiresIn(invitation.expires_at, t) })}
              tone={expired ? "text-accent-error" : undefined}
            />
          </div>

          <Separator />

          <div className="flex flex-col gap-2">
            <div className="flex items-baseline justify-between gap-3">
              <span className="text-sm font-medium text-ink tabular-nums">
                {t("approval.tally", "{approvals} of {threshold, plural, one {# approval} other {# approvals}} so far", { approvals, threshold })}
              </span>
              <span className="text-xs text-ink-soft tabular-nums">
                {t("approval.tallyOwners", "{owners, plural, one {# owner} other {# owners}} in total", { owners: invitation.owner_count ?? 0 })}
              </span>
            </div>
            {/* The sentence above states the tally; a second, unlabelled progressbar in the
                accessibility tree would only repeat it. */}
            <Progress value={progress} className="h-1.5" aria-hidden />
            {/* Kind-aware like the title: a change of terms or a NAV mark moves no money, and a
                hint that says it does would tell an owner they are approving the wrong thing. */}
            <p className="text-xs text-ink-soft">
              {(valuation ? t("approval.valuation.tallyHint", "More than half of the owners must approve before the fund is marked. You are one of them.") : feePolicy ? t("approval.feePolicy.tallyHint", "More than half of the owners must approve before the terms change. You are one of them.") : t("approval.tallyHint", "More than half of the owners must approve before any money moves. You are one of them."))}
            </p>
          </div>
        </CardContent>
      </Card>

      {burned ? (
        <ApprovalBurned
          description={valuation ? t("approval.valuation.burned.body", "The code was entered incorrectly too many times, so the link was closed and every owner has been told. Nothing was approved and the fund's NAV mark is unchanged. Ask whoever opened the request to send a new one.") : feePolicy ? t("approval.feePolicy.burned.body", "The code was entered incorrectly too many times, so the link was closed and every owner has been told. Nothing was approved and the product's terms are unchanged. Ask whoever opened the request to send a new one.") : undefined}
        />
      ) : expired ? (
        <ApprovalExpired />
      ) : settled ? (
        <ApprovalOutcome
          icon={settled === "approve" ? <CheckCircle2 /> : <XCircle />}
          tone={settled === "approve" ? "text-positive" : "text-ink-soft"}
          title={settled === "approve" ? words.approvedTitle : words.rejectedTitle}
          description={justDecided ? words.freshBody : t("approval.decided.body", "You answered this earlier, and that answer stands. An answer cannot be changed once it is given.")}
        />
      ) : (
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t("approval.decisionTitle", "Your decision")}</CardTitle>
            <CardDescription className="text-balance">{t("approval.decisionLead", "Both answers need the code from your email. Enter it once, then choose.")}</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <p className="text-xs text-ink-soft">{t("approval.votingAs", "You are answering as {email}.", { email: invitation.voter_email })}</p>

            <CodeField value={code} onChange={setCode} disabled={pending !== null} attemptsRemaining={rejectedAttempts} />

            {actionError !== null && rejectedAttempts === null && (
              <ResourceError message={stripTransportPrefix(errorMessage(actionError, t))} />
            )}

            {/* Two answers, deliberately unequal in weight as well as colour. Approving is
                the act this page exists for and carries the solid, full-width control;
                rejecting is a smaller outline button that has to be aimed at. Both are
                gated by the same code — a rejection stops a payout the fund's own owners
                asked for, so it is no more casual than an approval. */}
            <div className="flex flex-col gap-2.5 sm:flex-row">
              <Button
                size="lg"
                className="font-semibold sm:flex-1"
                disabled={code.trim().length === 0 || pending !== null}
                onClick={() => void decide("approve")}
              >
                {pending === "approve" && <Spinner aria-hidden />}
                {words.approve}
              </Button>
              <Button
                size="lg"
                variant="outline"
                className="border-accent-error/40 text-accent-error hover:bg-accent-error/10 hover:text-accent-error sm:shrink-0"
                disabled={code.trim().length === 0 || pending !== null}
                onClick={() => void decide("reject")}
              >
                {pending === "reject" && <Spinner aria-hidden />}
                {t("approval.reject", "Reject")}
              </Button>
            </div>
          </CardContent>
        </Card>
      )}

      <p className="text-center text-xs text-ink-soft">{t("approval.footnote", "This link was made for you alone. It works once, expires 72 hours after it was sent, and should not be forwarded.")}</p>
    </ApprovalPage>
  );
}

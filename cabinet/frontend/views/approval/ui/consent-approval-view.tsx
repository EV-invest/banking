"use client";

// "This is your money — do you agree to it moving?" — reached from an email, with no
// session and no way back.
//
// The same two facts from docs/CONSILIUM.md shape it as shape the payout approval: the
// GET is inert (policy 5), and the reader agrees to what they can see (policy 12–13). One
// thing is different and every word carries it — the money is the READER's, not the
// fund's. Nobody else is asked, nobody else can answer, and the initiator is named as the
// person asking, in the reader's own terms: who wants it, from where to where, and why.
//
// Everything counted — attempts, the settled answer — is read back from the server
// (`useConsentDecision`); this page never decrements, increments or predicts any of it.

import { CheckCircle2, XCircle } from "lucide-react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@evinvest/uikit";

import { ApprovalUnavailableError } from "@/entities/approval/api/approval-client";
import { consentApprovalResource } from "@/entities/approval/model/approval-resource";
import { expiresIn, formatMoment, hasExpired } from "@/shared/lib/datetime";
import { settledConsent } from "@/shared/lib/decision";
import { useResource } from "@/shared/lib/resource";
import { useConsentDecision } from "@/views/approval/model/use-consent-decision";
import {
  ApprovalBurned,
  ApprovalExpired,
  ApprovalOutcome,
  ApprovalPage,
  ApprovalSkeleton,
  ApprovalUnavailable,
  ApprovalUnreachable,
  ApprovalUnrenderable,
  DetailRow,
} from "@/views/approval/ui/approval-chrome";
import { ConsentDecisionCard } from "@/views/approval/ui/consent-decision";
import { PaymentTermsBlock, renderablePayment } from "@/views/approval/ui/payment-terms";

export function ConsentApprovalView({ token }: { token: string }) {
  const t = useT();
  const locale = useLocale();
  const summary = useResource(consentApprovalResource, token);
  const invitation = summary.data ?? null;

  const { pending, error, rejectedAttempts, justDecided, spent, decide } = useConsentDecision(token, invitation, summary.refresh);

  // NOT `?? null`: an unanswered seat arrives as the truthy string "pending" (`shared/lib/decision.ts`).
  const settled = settledConsent(invitation?.decision);
  // Once answered, the outcome stands: a background revalidation 404s the moment the token
  // is spent, and flipping "you agreed" into "this link is dead" would be alarming and wrong.
  const gone = spent || (!settled && summary.error instanceof ApprovalUnavailableError);

  if (gone) return <ApprovalPage><ApprovalUnavailable /></ApprovalPage>;
  if (!invitation) {
    return (
      <ApprovalPage>
        {summary.isLoading ? <ApprovalSkeleton /> : <ApprovalUnreachable onRetry={() => void summary.refresh()} retrying={summary.isValidating} />}
      </ApprovalPage>
    );
  }
  if (!renderablePayment(invitation)) {
    return <ApprovalPage><ApprovalUnrenderable onRetry={() => void summary.refresh()} retrying={summary.isValidating} /></ApprovalPage>;
  }

  const burned = !settled && (invitation.attempts_remaining ?? 0) <= 0;
  const expired = !settled && hasExpired(invitation.expires_at);

  return (
    <ApprovalPage>
      <Card>
        <CardHeader>
          <CardTitle className="text-xl">{t("consent.title")}</CardTitle>
          <CardDescription className="text-balance">{t("consent.lead", { initiator: invitation.initiator_email })}</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-5">
          <PaymentTermsBlock terms={invitation} payloadHash={invitation.payload_hash} reasonLabel={t("consent.reasonLabel", { initiator: invitation.initiator_email })} />
          <div className="flex flex-col gap-2.5">
            <DetailRow label={t("consent.askedBy")} value={invitation.initiator_email} />
            <DetailRow label={t("consent.yourMoney")} value={invitation.subject_email} />
            <DetailRow
              label={t("approval.expires")}
              value={t("approval.expiresValue", { at: formatMoment(invitation.expires_at, locale), left: expiresIn(invitation.expires_at, t) })}
              tone={expired ? "text-destructive" : undefined}
            />
          </div>
        </CardContent>
      </Card>

      {burned ? (
        <ApprovalBurned description={t("consent.burned.body")} />
      ) : expired ? (
        <ApprovalExpired />
      ) : settled ? (
        <ApprovalOutcome
          icon={settled === "approve" ? <CheckCircle2 /> : <XCircle />}
          tone={settled === "approve" ? "text-main-accent-t2" : "text-muted-foreground"}
          title={t(settled === "approve" ? "consent.decided.approvedTitle" : "consent.decided.rejectedTitle")}
          description={t(justDecided ? (settled === "approve" ? "consent.decided.approvedFresh" : "consent.decided.rejectedFresh") : "approval.decided.body")}
        />
      ) : (
        <ConsentDecisionCard email={invitation.subject_email} pending={pending} rejectedAttempts={rejectedAttempts} error={error} onDecide={decide} />
      )}

      <p className="text-center text-xs text-muted-foreground">{t("approval.footnote")}</p>
    </ApprovalPage>
  );
}

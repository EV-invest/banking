"use client";

// The last issuance, as the hub answered it — a mint or a hand-over of the company's
// stake, which share the `UnitIssuance` shape and differ in `source`. `queued` is the
// ordinary answer (the relay posts the leg after the POST returns), so it is shown as a
// state, not as a warning. No toast: the cabinet mounts no `Toaster`, and a result that
// names a queued money movement should stay beside the split it will change.

import { CheckCircle2, Clock } from "lucide-react";

import { useT } from "@evinvest/i18n/react";

import type { UnitIssuance } from "@/shared/contracts/admin";
import { formatUnits } from "@/shared/lib/money";

export interface IssuanceOutcome {
  issuance: UnitIssuance;
  /** Who the units landed on, as the operator picked them (an email, or "Company"). */
  holderLabel: string;
}

const COPY = {
  issue: { applied: "admin.alloc.issue.resultApplied", queued: "admin.alloc.issue.resultQueued" },
  transfer: { applied: "admin.alloc.transfer.resultApplied", queued: "admin.alloc.transfer.resultQueued" },
} as const;

export function IssuanceResult({ outcome, kind }: { outcome: IssuanceOutcome; kind: keyof typeof COPY }) {
  const t = useT();
  const applied = outcome.issuance.state === "applied";
  const args = { units: formatUnits(outcome.issuance.units), holder: outcome.holderLabel };
  return (
    <p className="flex items-start gap-2 text-xs text-main-accent-t2">
      {applied ? <CheckCircle2 className="mt-0.5 size-3.5 shrink-0" /> : <Clock className="mt-0.5 size-3.5 shrink-0" />}
      <span>
        {t(COPY[kind][applied ? "applied" : "queued"], args)}
        {/* The source the hub recorded, so a mint and a hand-over of the same figure to
            the same holder are told apart on screen and not only in the audit log. */}
        <span className="text-muted-foreground"> · {t(outcome.issuance.source === "company" ? "admin.alloc.issuance.source.company" : "admin.alloc.issuance.source.mint")}</span>
      </span>
    </p>
  );
}

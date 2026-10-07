"use client";

// The last issuance, as the hub answered it — a mint or a retirement, which share the
// `UnitIssuance` shape and differ in `source`. `queued` is the
// ordinary answer (the relay posts the leg after the POST returns), so it is shown as a
// state, not as a warning. No toast: the cabinet mounts no `Toaster`, and a result that
// names a queued money movement should stay beside the split it will change.

import { CheckCircle2, Clock } from "lucide-react";

import type { Translate } from "@evinvest/i18n";
import { useLocale, useT } from "@evinvest/i18n/react";

import type { UnitIssuance, UnitIssuanceSource } from "@/shared/contracts/admin";
import { formatUnits } from "@/shared/lib/money";
import { wordFor } from "@/shared/lib/wire-words";

export interface IssuanceOutcome {
  issuance: UnitIssuance;
  /** Who the units landed on, as the operator picked them (an email, usually). */
  holderLabel: string;
}

type Args = { units: string; holder: string };

const COPY = {
  issue: {
    applied: (t: Translate, args: Args) => t("admin.alloc.issue.resultApplied", "{units} units issued to {holder}.", args),
    queued: (t: Translate, args: Args) => t("admin.alloc.issue.resultQueued", "{units} units to {holder} queued — the relay posts the mint shortly, and the split below follows.", args),
  },
  retire: {
    applied: (t: Translate, args: Args) => t("admin.alloc.retire.resultApplied", "{units} units retired from {holder}.", args),
    queued: (t: Translate, args: Args) => t("admin.alloc.retire.resultQueued", "{units} units from {holder} queued for retirement — the relay posts the burn shortly, and the split below follows.", args),
  },
} as const;

const sources = (t: Translate): Record<UnitIssuanceSource, string> => ({
  mint: t("admin.alloc.issuance.source.mint", "minted"),
  retire: t("admin.alloc.issuance.source.retire", "retired"),
});

export function IssuanceResult({ outcome, kind }: { outcome: IssuanceOutcome; kind: keyof typeof COPY }) {
  const t = useT();
  const locale = useLocale();
  const applied = outcome.issuance.state === "applied";
  const args = { units: formatUnits(outcome.issuance.units, locale), holder: outcome.holderLabel };
  return (
    <p className="flex items-start gap-2 text-xs text-positive">
      {applied ? <CheckCircle2 className="mt-0.5 size-3.5 shrink-0" /> : <Clock className="mt-0.5 size-3.5 shrink-0" />}
      <span>
        {COPY[kind][applied ? "applied" : "queued"](t, args)}
        {/* The source the hub recorded, so a mint and a burn of the same figure on the
            same holder are told apart on screen and not only in the audit log. */}
        <span className="text-ink-soft"> · {wordFor(sources(t), outcome.issuance.source) ?? outcome.issuance.source}</span>
      </span>
    </p>
  );
}

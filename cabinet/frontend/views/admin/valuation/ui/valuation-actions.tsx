"use client";

// The two ways a mark leaves the form. Post records it now, inside the NAV-move guard.
// Propose puts it to the owners as a consilium — and that is the ONLY way past the guard:
// there is no per-request override any more, because one admin with a flag could lift the
// guard and drain pooled cash (banking#232). Both routes are gated alike, so the operator
// chooses where the figure goes, never whether the guard applies.

import { CheckCircle2, Loader2 } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, AlertTitle, Button } from "@evinvest/uikit";

import { postValuation, proposeValuationOverride } from "@/entities/admin/api/admin-client";
import type { FundNav } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { formatExactUsdt } from "@/shared/lib/money";
import { TipAnchor } from "@/shared/tips";
import { Link } from "@/shared/ui/cabinet-link";
import { PostedMark } from "@/views/admin/valuation/ui/posted-mark";

const TEAL_CTA = "bg-primary text-on-primary hover:bg-primary/90";

type Route = "post" | "propose";

export function ValuationActions({
  service,
  aum,
  disabled,
  onPosted,
  onProposed,
  onError,
}: {
  service: string;
  aum: string;
  disabled: boolean;
  /** The POST answers with the new mark; the caller publishes it and clears the form. */
  onPosted: (nav: FundNav) => Promise<void> | void;
  onProposed: () => void;
  onError: (message: string | null) => void;
}) {
  const t = useT();
  const [busy, setBusy] = useState<Route | null>(null);
  // What was proposed, captured at the click: the caller clears the AUM field on success,
  // so the receipt cannot read it back from the form.
  const [proposed, setProposed] = useState<{ service: string; aum: string } | null>(null);
  // The mark as the hub answered it. Kept whole (it names its own `service`) so switching
  // funds hides it by comparison instead of an effect that races the select.
  const [posted, setPosted] = useState<FundNav | null>(null);

  const run = async (route: Route) => {
    setBusy(route);
    onError(null);
    try {
      if (route === "post") {
        const mark = await postValuation({ service, aum });
        setProposed(null);
        setPosted(mark);
        await onPosted(mark);
      } else {
        await proposeValuationOverride({ service, aum });
        setPosted(null);
        setProposed({ service, aum });
        onProposed();
      }
    } catch (e) {
      onError(errorMessage(e, t));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      {posted?.service === service && <PostedMark mark={posted} onClose={() => setPosted(null)} />}

      {proposed && (
        // Leads with PROPOSED, not "done": nothing is marked until the vote carries, and
        // the room with the live tally is one link away (same receipt as a payment).
        <Alert role="status" className="border-positive/40 bg-positive/10">
          <CheckCircle2 className="size-4 text-positive" />
          <AlertTitle>{t("admin.valuation.proposedTitle")}</AlertTitle>
          <AlertDescription className="gap-3 text-ink">
            <p className="leading-relaxed tabular-nums">
              {t("admin.valuation.proposedBody", { service: proposed.service, aum: formatExactUsdt(proposed.aum) })}
            </p>
            <div className="flex flex-wrap gap-2">
              <Button asChild size="sm" variant="outline">
                <Link href="/consilium">{t("admin.valuation.openConsilium")}</Link>
              </Button>
              <Button type="button" size="sm" variant="ghost" onClick={() => setProposed(null)}>
                {t("ui.close")}
              </Button>
            </div>
          </AlertDescription>
        </Alert>
      )}

      <div className="flex flex-wrap items-center justify-end gap-3">
        <p className="min-w-48 flex-1 text-xs text-ink-soft">{t("admin.valuation.proposeHint")}</p>
        {/* i18n-max: 24 per verb — both Buttons are `shrink-0` in a wrapping row. */}
        <span className="inline-flex shrink-0 items-center gap-1.5">
          <Button type="button" variant="outline" disabled={disabled || busy !== null} onClick={() => void run("propose")}>
            {busy === "propose" ? <Loader2 className="size-4 animate-spin" /> : null}
            {t("admin.valuation.propose")}
          </Button>
          <TipAnchor anchor="admin.valuation.post.propose" />
        </span>
        <Button type="button" className={TEAL_CTA} disabled={disabled || busy !== null} onClick={() => void run("post")}>
          {busy === "post" ? <Loader2 className="size-4 animate-spin" /> : null}
          {t("admin.valuation.postValuation")}
        </Button>
      </div>
    </div>
  );
}

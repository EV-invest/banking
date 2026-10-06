"use client";

import { CheckCircle2, TriangleAlert } from "lucide-react";
import { useCallback, useId, useState } from "react";

import type { Translate } from "@evinvest/i18n";
import { useLocale, useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, AlertTitle, Button, Card, CardContent, Field, FieldLabel, Input, Spinner } from "@evinvest/uikit";

import { recordTreasuryDeposit, type RecordedArrival } from "@/entities/admin/api/admin-client";
import type { RailLiquidity } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { StaggerItem } from "@/shared/ui/motion";
import { RichMessage } from "@/shared/ui/rich-message";
import { formatUsdt } from "@/views/admin/lib/format";
import { RailSelect, watchedRails } from "@/views/admin/treasury/ui/rail-select";

/** Funding a treasury hot wallet directly moves real USDT while writing nothing to the
 *  ledger: the rail's custody figure doesn't move and the dispatch gate (`min(TB rail,
 *  on-chain treasury)`) keeps reading the old number, so that liquidity can't be
 *  withdrawn. This is where an arrival that is a PERSON's deposit gets recorded; one that
 *  is nobody's yet is a seed proposal (`SeedCapitalForm`), which the hub refuses here.
 *
 *  Idempotent by `tx_ref`, so the honest outcome is three-way: credited, already credited,
 *  or failed — collapsing "already credited" into a generic success would invite the
 *  operator to re-submit under a second reference and double-count the same dollar. */
export function RecordArrival({ rails, onRecorded }: { rails: RailLiquidity[] | undefined; onRecorded: () => void }) {
  const t = useT();
  const locale = useLocale();
  const id = useId();
  const [network, setNetwork] = useState("");
  const [txRef, setTxRef] = useState("");
  const [amount, setAmount] = useState("");
  const [state, setState] = useState<{ busy: boolean; error: string | null; result: RecordedArrival | null }>({ busy: false, error: null, result: null });

  const submit = useCallback(() => {
    setState({ busy: true, error: null, result: null });
    // The amount goes as an ASSERTION, and only when the operator typed one — the hub reads
    // the real figure off the chain. Sending it as a value is what would let this mint money.
    recordTreasuryDeposit({ tx_ref: txRef.trim(), network, expected_amount: amount.trim() || undefined })
      .then((res) => {
        setState({ busy: false, error: null, result: res });
        if (res.recorded) {
          setTxRef("");
          setAmount("");
          onRecorded();
        }
      })
      .catch((e: Error) => setState({ busy: false, error: errorMessage(e, t), result: null }));
  }, [txRef, network, amount, onRecorded, t]);

  return (
    <StaggerItem as="section" className="space-y-3">
      <p className="text-xs font-semibold uppercase tracking-widest text-ink-soft">{t("admin.treasury.recordArrival", "Record an out-of-band arrival")}</p>
      <Card>
        <CardContent className="space-y-5 py-6">
          <p className="max-w-3xl text-sm text-ink-soft">{t("admin.treasury.recordArrivalIntro", "USDT sent straight to a treasury hot wallet is real money the ledger never saw. Give its on-chain reference and the hub reads the transfer back off the chain — the amount and the party credited come from there, so this records an arrival rather than asserting one.")}</p>
          <div className="grid gap-4 md:grid-cols-3">
            <RailSelect value={network} options={watchedRails(rails)} onChange={setNetwork} />
            <Field>
              <FieldLabel htmlFor={`${id}-amount`}>{t("admin.treasury.expectedAmount", "Expected amount (USDT) · optional")}</FieldLabel>
              <Input id={`${id}-amount`} value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="decimal" placeholder={t("admin.treasury.placeholder.any", "any")} className="tabular-nums" />
            </Field>
            <Field>
              <FieldLabel htmlFor={`${id}-ref`}>{t("admin.treasury.onchainRef", "On-chain reference")}</FieldLabel>
              {/* A format literal, not prose — it reads the same in every locale. */}
              <Input id={`${id}-ref`} value={txRef} onChange={(e) => setTxRef(e.target.value)} placeholder="0xhash:logIndex" spellCheck={false} className="font-mono-tech" />
            </Field>
          </div>

          {/* The two reference formats are code, so they ride in as ICU arguments and the
              note stays one key: a translator needs the whole sentence to place them, and
              splitting around the two spans would hand them three fragments instead. */}
          <p className="text-xs text-ink-soft">
            <RichMessage
              format={(t, values) => t("admin.treasury.refNote", "The reference is both what gets verified and the idempotency key — {evmRef} on an EVM rail, {tonRef} on TON. A reference that names no confirmed transfer to one of our addresses is refused, and a re-submission of one already recorded is a no-op. Fill the amount only to assert what you expect; a mismatch is then an error instead of a surprise.", values)}
              values={{
                evmRef: <code className="font-mono-tech">txhash:logIndex</code>,
                tonRef: <code className="font-mono-tech">txhash:piggybank</code>,
              }}
            />
          </p>

          {state.error && (
            <Alert variant="destructive">
              <TriangleAlert className="size-4" />
              <AlertDescription>{state.error}</AlertDescription>
            </Alert>
          )}
          {state.result?.recorded && (
            <Alert role="status" variant="success">
              <CheckCircle2 className="size-4" />
              <AlertTitle>{t("admin.treasury.recordedTitle", "Recorded")}</AlertTitle>
              <AlertDescription>{t("admin.treasury.recorded", "Recorded — the chain reported {amount}, credited to {party}.", { amount: `${formatUsdt(state.result.amount, locale)} USDT`, party: partyLabel(state.result, t) })}</AlertDescription>
            </Alert>
          )}
          {state.result && !state.result.recorded && (
            // A successful no-op, not a failure — so neither the success nor the error tone.
            <Alert role="status">
              <AlertDescription>{t("admin.treasury.alreadyRecorded", "Already recorded — that reference was credited before, nothing changed.")}</AlertDescription>
            </Alert>
          )}

          <Button type="button" className="ml-auto flex" disabled={state.busy || !network || !txRef.trim()} onClick={submit}>
            {state.busy ? <Spinner aria-hidden /> : null}
            {t("admin.treasury.recordArrivalSubmit", "Record arrival")}
          </Button>
        </CardContent>
      </Card>
    </StaggerItem>
  );
}

/** Who the chain said the money belongs to — always a person now (#245); the hub refuses
 *  a transfer that is nobody's and points at the seed proposal instead. Shown rather than
 *  assumed so the operator sees whose deposit was actually credited. */
function partyLabel({ party_kind, party_id }: RecordedArrival, t: Translate): string {
  return party_id ? t("admin.treasury.party.generic", "{kind} {id}", { kind: party_kind, id: party_id }) : party_kind;
}

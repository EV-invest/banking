"use client";

import { type FormEvent, useId, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Field, FieldLabel, Input, Spinner } from "@evinvest/uikit";

// What may be granted is the tenant's catalog and the caller's delegation, neither of which
// the cabinet can read, so the target is typed and the plane refuses what it does not
// allow. The roster's own targets are offered, since they are what gets granted again.
export function GrantForm({ namespace, known, busy, onSubmit }: { namespace: string; known: readonly string[]; busy: boolean; onSubmit: (email: string, target: string) => Promise<boolean> }) {
  const t = useT();
  const id = useId();
  const [email, setEmail] = useState("");
  const [target, setTarget] = useState("");
  const trimmedEmail = email.trim();
  const trimmedTarget = target.trim();

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!trimmedEmail || !trimmedTarget || busy) return;
    if (await onSubmit(trimmedEmail, trimmedTarget)) setEmail("");
  };

  return (
    <form onSubmit={submit} className="flex flex-col gap-3 rounded-lg border border-border bg-secondary p-3">
      <Field>
        <FieldLabel htmlFor={`${id}-email`}>{t("panelAccess.field.email")}</FieldLabel>
        <Input id={`${id}-email`} type="email" autoComplete="off" value={email} onChange={(e) => setEmail(e.target.value)} className="w-full" />
      </Field>
      <Field>
        <FieldLabel htmlFor={`${id}-target`}>{t("panelAccess.field.target")}</FieldLabel>
        <Input
          id={`${id}-target`}
          list={`${id}-known`}
          autoComplete="off"
          spellCheck={false}
          placeholder={`${namespace}:operator`}
          value={target}
          onChange={(e) => setTarget(e.target.value)}
          className="w-full font-mono"
        />
        <datalist id={`${id}-known`}>
          {known.map((k) => (
            <option key={k} value={k} />
          ))}
        </datalist>
      </Field>
      <Button type="submit" className="w-full" disabled={busy || !trimmedEmail || !trimmedTarget}>
        {busy ? <Spinner aria-hidden /> : null}
        {t("panelAccess.grant")}
      </Button>
    </form>
  );
}

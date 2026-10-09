"use client";

import { type FormEvent, useId, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Field, FieldLabel, Input, Spinner } from "@evinvest/uikit";

import { type PickedUser, UserPicker } from "@/features/user-picker/ui/user-picker";
import { useSession } from "@/shared/lib/use-session";

// What may be granted is the tenant's catalog and the caller's delegation, neither of which
// the cabinet can read, so the target is typed and the plane refuses what it does not
// allow. The roster's own targets are offered, since they are what gets granted again.
// Only the console can read the directory to pick from; a delegate names whom they know.
// A prefilled email (the panel's access-request ping) is typed, as the ping carries no user id to pick.
export type GrantPrefill = { email?: string; target?: string };

export function GrantForm({ namespace, prefill, known, busy, onSubmit }: { namespace: string; prefill?: GrantPrefill; known: readonly string[]; busy: boolean; onSubmit: (email: string, target: string) => Promise<boolean> }) {
  const t = useT();
  const id = useId();
  const canPick = useSession()?.user?.isAdmin === true && !prefill?.email;
  const [typed, setTyped] = useState(prefill?.email ?? "");
  const [picked, setPicked] = useState<PickedUser | null>(null);
  const [target, setTarget] = useState(prefill?.target ?? "");
  const trimmedEmail = (canPick ? (picked?.email ?? "") : typed).trim();
  const trimmedTarget = target.trim();

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!trimmedEmail || !trimmedTarget || busy) return;
    if (await onSubmit(trimmedEmail, trimmedTarget)) {
      setTyped("");
      setPicked(null);
    }
  };

  return (
    <form onSubmit={submit} className="flex flex-col gap-3 rounded-lg border border-border bg-secondary p-3">
      <Field>
        {canPick ? (
          <>
            <FieldLabel id={`${id}-email`}>{t("panelAccess.field.email", "Email")}</FieldLabel>
            <UserPicker value={picked} onPick={setPicked} labelledBy={`${id}-email`} />
          </>
        ) : (
          <>
            <FieldLabel htmlFor={`${id}-email`}>{t("panelAccess.field.email", "Email")}</FieldLabel>
            <Input id={`${id}-email`} type="email" autoComplete="off" value={typed} onChange={(e) => setTyped(e.target.value)} className="w-full" />
          </>
        )}
      </Field>
      <Field>
        <FieldLabel htmlFor={`${id}-target`}>{t("panelAccess.field.target", "What to grant")}</FieldLabel>
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
        {t("panelAccess.grant", "Grant access")}
      </Button>
    </form>
  );
}

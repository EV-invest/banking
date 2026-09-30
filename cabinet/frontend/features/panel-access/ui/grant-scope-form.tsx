"use client";

import { type FormEvent, useId, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Field, FieldLabel, Input, Select, SelectContent, SelectItem, SelectTrigger, Spinner } from "@evinvest/uikit";

import type { ScopeRole } from "@/entities/scope/lib/access";

// Only the roles the caller may hand out: a scope's own admin never sees `admin` here,
// since the plane refuses them minting or promoting one.
export function GrantScopeForm({ roles, busy, onSubmit }: { roles: readonly ScopeRole[]; busy: boolean; onSubmit: (email: string, role: ScopeRole) => Promise<boolean> }) {
  const t = useT();
  const id = useId();
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<ScopeRole>("operator");
  const trimmed = email.trim();

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!trimmed || busy) return;
    if (await onSubmit(trimmed, role)) setEmail("");
  };

  return (
    <form onSubmit={submit} className="flex flex-col gap-3 rounded-lg border border-border bg-secondary p-3">
      <Field>
        <FieldLabel htmlFor={`${id}-email`}>{t("panelAccess.field.email")}</FieldLabel>
        <Input id={`${id}-email`} type="email" autoComplete="off" value={email} onChange={(e) => setEmail(e.target.value)} className="w-full" />
      </Field>
      {roles.length > 1 && (
        <Field>
          <FieldLabel htmlFor={`${id}-role`}>{t("panelAccess.field.role")}</FieldLabel>
          <Select value={role} onValueChange={(v) => setRole(roles.find((r) => r === v) ?? role)}>
            <SelectTrigger id={`${id}-role`} className="w-full border-border bg-secondary">
              <span className="truncate">{t(`admin.role.${role}`)}</span>
            </SelectTrigger>
            <SelectContent>
              {roles.map((r) => (
                <SelectItem key={r} value={r}>
                  {t(`admin.role.${r}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
      )}
      <Button type="submit" className="w-full" disabled={busy || !trimmed}>
        {busy ? <Spinner aria-hidden /> : null}
        {roles.length > 1 ? t("panelAccess.grant") : t("panelAccess.grantOperator")}
      </Button>
    </form>
  );
}

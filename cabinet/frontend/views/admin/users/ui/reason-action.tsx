"use client";

// A button that asks for a sentence before it does anything.
//
// Shared by the two controls in the user drawer that write to the audit trail — the hold
// and the three proposals — because every one of them is refused upstream without a reason
// and the shape of "ask, then send" is identical. What is NOT shared is which verb each
// one uses or what it then calls: this takes a label, a hint and an `onSubmit`, and knows
// nothing about holds, suspensions or roles.

import { Loader2 } from "lucide-react";
import type { ReactNode } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Field, FieldDescription, FieldLabel, Textarea } from "@evinvest/uikit";

export interface ReasonActionProps {
  open: boolean;
  onOpen: () => void;
  onCancel: () => void;
  label: string;
  /** What this particular action does, in the reader's terms. */
  hint: string;
  icon: ReactNode;
  /** Destructive tone for the one action that takes something away immediately. */
  destructive?: boolean;
  busy: boolean;
  reason: string;
  setReason: (value: string) => void;
  /** Unique per mounted action, so the label binds to its own field. */
  inputId: string;
  onSubmit: () => void;
}

/**
 * A button that asks for a sentence before it does anything.
 *
 * Inline rather than a modal, following the owners' room: the reason is part of the record
 * the reader is composing, not a confirmation of something already decided — and a 340px
 * drawer has no room for a dialog that would cover the account it is about. Submit stays
 * dark until the field has content, because every one of these is refused upstream without
 * a reason, and a button that only produces an error is worse than one that waits.
 */
export function ReasonAction({
  open,
  onOpen,
  onCancel,
  label,
  hint,
  icon,
  destructive = false,
  busy,
  reason,
  setReason,
  inputId,
  onSubmit,
}: ReasonActionProps) {
  const t = useT();

  if (!open) {
    return (
      <Button
        type="button"
        variant="outline"
        size="sm"
        className={destructive ? "w-full border-destructive/40 text-destructive hover:bg-destructive/10" : "w-full"}
        disabled={busy}
        onClick={onOpen}
      >
        {icon}
        {label}
      </Button>
    );
  }

  return (
    <Field>
      <FieldLabel htmlFor={inputId}>{t("admin.users.reasonLabel")}</FieldLabel>
      <Textarea
        id={inputId}
        value={reason}
        onChange={(e) => setReason(e.target.value)}
        rows={3}
        maxLength={1000}
        placeholder={t("admin.users.reasonPlaceholder")}
      />
      <FieldDescription>{hint}</FieldDescription>
      <FieldDescription>{t("admin.users.reasonHint")}</FieldDescription>
      <div className="flex gap-2">
        <Button type="button" variant="outline" size="sm" className="flex-1" disabled={busy || reason.trim().length === 0} onClick={onSubmit}>
          {busy ? <Loader2 className="size-3.5 animate-spin" /> : icon}
          {label}
        </Button>
        <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={onCancel}>
          {t("ui.cancel")}
        </Button>
      </div>
    </Field>
  );
}

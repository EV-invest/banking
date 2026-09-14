"use client";

// The form's atoms: a rate field, a closed-vocabulary chooser, and a labelled figure.

import { useId } from "react";

import { useT } from "@evinvest/i18n/react";
import { Field, FieldDescription, FieldLabel, InputGroup, InputGroupAddon, InputGroupInput, InputGroupText, ToggleGroup, ToggleGroupItem } from "@evinvest/uikit";

/** A rate, as a term sheet states one. The percent sign is furniture inside the field
 *  rather than a character the operator types, so what the value means is legible while
 *  the box still holds nothing but the number `toBps` parses. */
export function PercentField({ label, value, onChange, hint, disabled }: { label: string; value: string; onChange: (v: string) => void; hint: string; disabled?: boolean }) {
  const id = useId();
  return (
    <Field>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <InputGroup>
        {/* `decimal` rather than `numeric`: half a percent is a rate someone will charge,
            and a numeric keypad on a phone has no decimal separator. */}
        <InputGroupInput id={id} inputMode="decimal" value={value} onChange={(e) => onChange(e.target.value)} disabled={disabled} className="tabular-nums" aria-describedby={`${id}-hint`} />
        {/* Not decorative: the unit is the whole point of this screen's change, and a
            reader who cannot see it is the one who most needs telling. */}
        <InputGroupAddon align="inline-end">
          <InputGroupText>%</InputGroupText>
        </InputGroupAddon>
      </InputGroup>
      <FieldDescription id={`${id}-hint`}>{hint}</FieldDescription>
    </Field>
  );
}

/** `value` is the wire enum the money plane stores; `labelKey` is only what a reader sees.
 *  Option lists live at module scope, where no hook can run, so they carry the key and the
 *  items resolve it against the reader's locale. */
export function Choice<T extends string>({
  label,
  value,
  onChange,
  options,
  disabled,
}: {
  label: string;
  value: string;
  onChange: (v: T) => void;
  options: readonly { value: T; labelKey: string }[];
  disabled?: boolean;
}) {
  const t = useT();
  const labelId = useId();
  return (
    <Field>
      {/* No `htmlFor`: the group is several buttons, and a label pointing at one of them
          would make the caption a click target for a single option. */}
      <FieldLabel id={labelId}>{label}</FieldLabel>
      <ToggleGroup
        type="single"
        variant="outline"
        size="sm"
        value={value}
        aria-labelledby={labelId}
        className="flex-wrap"
        // A single-select group hands back a string; an empty one means the pressed item
        // was pressed again, and a term cannot be "none", so that click changes nothing.
        onValueChange={(next) => {
          if (typeof next === "string" && next !== "") onChange(next as T);
        }}
      >
        {options.map((option) => (
          <ToggleGroupItem key={option.value} value={option.value} disabled={disabled} className="text-xs">
            {t(option.labelKey)}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
    </Field>
  );
}

export function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="font-medium tabular-nums">{value}</dd>
    </div>
  );
}

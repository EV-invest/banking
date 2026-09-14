"use client";

// The form's atoms: a rate field, a closed-vocabulary chooser, and a labelled figure.

import { Input } from "@evinvest/uikit";

import { useT } from "@evinvest/i18n/react";

/** A rate, as a term sheet states one. The percent sign is furniture inside the field
 *  rather than a character the operator types, so what the value means is legible while
 *  the box still holds nothing but the number `toBps` parses. */
export function PercentField({ label, value, onChange, hint, disabled }: { label: string; value: string; onChange: (v: string) => void; hint: string; disabled?: boolean }) {
  return (
    <label className="space-y-1.5 text-sm">
      <span className="block font-medium">{label}</span>
      <div className="relative">
        {/* `decimal` rather than `numeric`: half a percent is a rate someone will charge,
            and a numeric keypad on a phone has no decimal separator. */}
        <Input inputMode="decimal" value={value} onChange={(e) => onChange(e.target.value)} disabled={disabled} className="pr-8 tabular-nums" />
        {/* Inside the `label`, so it joins the field's accessible name — "Management %
            per year". Not decorative: the unit is the whole point of this screen's
            change, and a reader who cannot see it is the one who most needs telling. */}
        <span className="pointer-events-none absolute inset-y-0 right-3 flex items-center text-sm text-muted-foreground">%</span>
      </div>
      <span className="block text-xs text-muted-foreground">{hint}</span>
    </label>
  );
}

/** `value` is the wire enum the money plane stores; `labelKey` is only what a reader sees.
 *  Option lists live at module scope, where no hook can run, so they carry the key and the
 *  chips resolve it against the reader's locale. */
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
  return (
    <div className="space-y-1.5 text-sm">
      <span className="block font-medium">{label}</span>
      <div className="flex flex-wrap gap-2">
        {options.map((option) => (
          <button
            key={option.value}
            type="button"
            onClick={() => onChange(option.value)}
            aria-pressed={value === option.value}
            disabled={disabled}
            className={`rounded-md border px-2.5 py-1.5 text-xs transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 ${
              value === option.value ? "border-main-accent-t1 bg-main-accent-t1/10" : "border-border hover:bg-muted/50"
            }`}
          >
            {t(option.labelKey)}
          </button>
        ))}
      </div>
    </div>
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

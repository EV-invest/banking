"use client";

// One end of the order: a kind, and — for the two kinds that name one of many — which one.
//
// Self-contained on purpose: the product list and the user search are read here rather
// than threaded through the form, so the form composes two of these with three props each
// instead of forwarding two resources it never looks at itself.

import { useT } from "@evinvest/i18n/react";
import { Button, Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle, Select, SelectContent, SelectItem, SelectTrigger, Skeleton } from "@evinvest/uikit";

import { adminAllocationsResource } from "@/entities/admin/model/admin-resource";
import { useResource } from "@/shared/lib/resource";
import { Link } from "@/shared/ui/cabinet-link";
import { ProductIcon } from "@/shared/ui/icons/products";
import { ResourceError } from "@/shared/ui/resource-error";
import { type EndDraft, type EndKind, END_KINDS } from "@/views/admin/payments/lib/terms";
import { endKindLabel } from "@/views/admin/payments/lib/words";
import { ExternalFields } from "@/views/admin/payments/ui/external-fields";
import { UserSearch } from "@/views/admin/payments/ui/user-search";

export function EndPicker({
  label,
  value,
  onChange,
  kinds,
}: {
  label: string;
  value: EndDraft;
  onChange: (next: EndDraft) => void;
  kinds: readonly EndKind[];
}) {
  const t = useT();
  const set = (patch: Partial<EndDraft>) => onChange({ ...value, ...patch });
  return (
    <fieldset className="min-w-0 space-y-2">
      <legend className="mb-2 text-xs font-medium text-muted-foreground">{label}</legend>
      {/* The id is cleared with the kind: a product slug is not a user id, and keeping one
          across the switch would send a name on the wrong claim. */}
      <Select value={value.kind} onValueChange={(next) => set({ kind: END_KINDS.find((k) => k === next) ?? value.kind, id: "", name: "" })}>
        <SelectTrigger className="w-full border-border bg-main-surface">
          <span className="truncate">{endKindLabel(value.kind, t)}</span>
        </SelectTrigger>
        <SelectContent>
          {kinds.map((kind) => (
            <SelectItem key={kind} value={kind}>
              {endKindLabel(kind, t)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {value.kind === "service" && <ProductSelect value={value.id} onChange={(id, name) => set({ id, name })} />}
      {value.kind === "user" && <UserSearch value={value.id} onChange={(id, name) => set({ id, name })} />}
      {value.kind === "external" && <ExternalFields network={value.network} address={value.address} onChange={set} />}
    </fieldset>
  );
}

/** Every registered product, drafts and closed ones included — a closed product still holds a claim. */
function ProductSelect({ value, onChange }: { value: string; onChange: (id: string, name: string) => void }) {
  const t = useT();
  const allocations = useResource(adminAllocationsResource);
  const products = allocations.data?.allocations ?? null;
  // A read that failed is not an empty registry, and a skeleton is not an answer either.
  if (!products && allocations.error) return <ResourceError error={allocations.error} onRetry={() => void allocations.refresh()} retrying={allocations.isValidating} />;
  if (!products) return <Skeleton className="h-9 w-full" />;
  if (products.length === 0) {
    return (
      <Empty className="border p-4">
        <EmptyHeader>
          <EmptyTitle className="text-sm">{t("admin.payments.noProducts")}</EmptyTitle>
          <EmptyDescription className="text-xs">{t("admin.payments.noProductsHint")}</EmptyDescription>
        </EmptyHeader>
        <EmptyContent>
          <Button asChild size="sm" variant="outline">
            <Link href="/admin/allocations">{t("nav.allocations")}</Link>
          </Button>
        </EmptyContent>
      </Empty>
    );
  }
  const picked = products.find((p) => p.service === value);
  return (
    <Select value={value} onValueChange={(next) => onChange(next, products.find((p) => p.service === next)?.title ?? next)}>
      <SelectTrigger className="w-full border-border bg-main-surface">
        <span className="flex min-w-0 items-center gap-2">
          {picked && <ProductIcon icon={picked.icon} className="size-4 shrink-0" />}
          <span className="truncate">{picked ? picked.title : t("admin.payments.pickProduct")}</span>
        </span>
      </SelectTrigger>
      <SelectContent>
        {products.map((p) => (
          <SelectItem key={p.service} value={p.service}>
            <span className="flex items-center gap-2">
              <ProductIcon icon={p.icon} className="size-4 shrink-0" />
              {p.title}
              <span className="font-mono-tech text-xs text-muted-foreground">{p.service}</span>
            </span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

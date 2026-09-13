"use client";

// Find the investor whose claim one end of the order is.
//
// A search rather than a select: the user list is paginated and can run to thousands, and
// the operator already knows the address they are after. The results are the same read
// the Users screen makes, keyed per query, so retyping a search answers from cache.
//
// What is kept once a row is picked is the CONCIERGE id and the email beside it — the id
// because that is what `Party.id` carries, the email so the review step can name the
// person rather than the id (`EndDraft.name`).

import { Check } from "lucide-react";
import { useId, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Empty, EmptyDescription, EmptyHeader, EmptyTitle, Field, FieldLabel, Input, Skeleton } from "@evinvest/uikit";

import { usersResource } from "@/entities/admin/model/admin-resource";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { ResourceError } from "@/shared/ui/resource-error";

/** Enough to disambiguate an address; a longer list means the query is not specific yet. */
const RESULT_LIMIT = 6;

export function UserSearch({ value, onChange }: { value: string; onChange: (id: string, email: string) => void }) {
  const t = useT();
  const inputId = useId();
  const listId = `${inputId}-results`;
  const [query, setQuery] = useState("");
  const [pickedEmail, setPickedEmail] = useState("");
  const list = useResource(usersResource, { query: query.trim() || undefined, limit: RESULT_LIMIT });
  const users = list.data?.users ?? null;

  if (value) {
    return (
      <div className="flex items-center justify-between gap-3 rounded-lg border border-border bg-main-surface px-3 py-2">
        <span className="flex min-w-0 items-center gap-2 text-sm">
          <Check className="size-4 shrink-0 text-main-accent-t2" />
          <span className="truncate">{pickedEmail || value}</span>
        </span>
        <Button type="button" size="sm" variant="ghost" onClick={() => onChange("", "")}>
          {t("ui.edit")}
        </Button>
      </div>
    );
  }

  return (
    <Field>
      <FieldLabel htmlFor={inputId}>{t("admin.payments.investor")}</FieldLabel>
      <Input
        id={inputId}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder={t("admin.payments.placeholder.userSearch")}
        spellCheck={false}
        autoComplete="off"
        aria-controls={listId}
      />
      {!users && list.error ? (
        <ResourceError error={list.error} onRetry={() => void list.refresh()} retrying={list.isValidating} />
      ) : !users ? (
        <Skeleton className="h-16 w-full" />
      ) : users.length === 0 ? (
        <Empty className="border p-4">
          <EmptyHeader>
            <EmptyTitle className="text-sm">{t("admin.payments.noUsers")}</EmptyTitle>
            <EmptyDescription className="text-xs">{t("admin.payments.noUsersHint")}</EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        // Not the uikit `Command`: it filters its items client-side by their `value`, and
        // these rows are already the server's answer to the query — a match on anything
        // but the email would vanish from the list. The listbox semantics are kept by hand.
        <ul id={listId} role="listbox" aria-label={t("admin.payments.investor")} className="divide-y divide-border rounded-lg border border-border">
          {users.map((u) => (
            <li key={u.user_id} role="presentation">
              <button
                type="button"
                role="option"
                aria-selected={false}
                onClick={() => {
                  setPickedEmail(u.email);
                  onChange(u.user_id, u.email);
                }}
                className={cn(
                  "flex w-full items-center justify-between gap-3 px-3 py-2 text-left text-sm outline-none transition-colors",
                  "hover:bg-foreground/5 focus-visible:ring-2 focus-visible:ring-ring",
                )}
              >
                <span className="min-w-0 truncate">{u.email || u.user_id}</span>
                <span className="shrink-0 font-mono-tech text-xs text-muted-foreground">{u.user_id.slice(0, 8)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </Field>
  );
}

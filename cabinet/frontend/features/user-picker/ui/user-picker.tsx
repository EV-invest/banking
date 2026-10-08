"use client";

// A searchable investor picker for the console's forms — grants, mints, holder grants, seed
// proposals and panel access name a person the same way. The candidates are the whole live
// directory, fuzzy-matched by `Command` on email and id: an operator granting access is very
// often reaching for someone they have never opened a row for before.

import { Check, ChevronsUpDown } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, Popover, PopoverContent, PopoverTrigger, Skeleton } from "@evinvest/uikit";

import { userDirectoryResource } from "@/entities/admin/model/admin-resource";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { ResourceError } from "@/shared/ui/resource-error";

export interface PickedUser {
  userId: string;
  email: string;
}

/** `labelledBy` is the id of the `FieldLabel` above the picker: the trigger is a button
 *  a `htmlFor` cannot reach through the popover, so the caption names it by reference. */
export function UserPicker({ value, onPick, labelledBy }: { value: PickedUser | null; onPick: (user: PickedUser) => void; labelledBy?: string }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const directory = useResource(userDirectoryResource);
  const users = directory.data;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button type="button" variant="outline" role="combobox" aria-expanded={open} aria-labelledby={labelledBy} className="w-full justify-between font-normal">
          <span className="min-w-0 truncate">{value ? value.email || value.userId : t("admin.alloc.grants.pickUser", "Search investors…")}</span>
          <ChevronsUpDown className="size-4 shrink-0 opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-72 p-0" align="start">
        <Command>
          <CommandInput placeholder={t("admin.users.searchPlaceholder", "Search email or user id…")} />
          <CommandList>
            {!users && directory.error ? (
              <ResourceError error={directory.error} onRetry={() => void directory.refresh()} retrying={directory.isValidating} />
            ) : !users ? (
              <Skeleton className="m-1 h-16" />
            ) : (
              <>
                <CommandEmpty>{t("admin.alloc.grants.noUserMatch", "No investors match")}</CommandEmpty>
                <CommandGroup>
                  {users.map((u) => (
                    <CommandItem
                      key={u.user_id}
                      value={`${u.email} ${u.user_id}`}
                      onSelect={() => {
                        onPick({ userId: u.user_id, email: u.email });
                        setOpen(false);
                      }}
                    >
                      <Check className={cn("size-4 shrink-0", value?.userId === u.user_id ? "opacity-100" : "opacity-0")} />
                      <span className="min-w-0 truncate">{u.email || u.user_id}</span>
                    </CommandItem>
                  ))}
                </CommandGroup>
              </>
            )}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

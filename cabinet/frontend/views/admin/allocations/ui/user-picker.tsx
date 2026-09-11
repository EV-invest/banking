"use client";

// A searchable investor picker for the grant form. The candidate list is the live
// `/api/admin/users` directory (the same read the Users screen searches), not a fixed
// roster — an operator granting access is very often reaching for someone they have never
// opened a row for before.

import { Check, ChevronsUpDown } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, Popover, PopoverContent, PopoverTrigger } from "@evinvest/uikit";

import { usersResource } from "@/entities/admin/model/admin-resource";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";

export interface PickedUser {
  userId: string;
  email: string;
}

export function UserPicker({ value, onPick }: { value: PickedUser | null; onPick: (user: PickedUser) => void }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  // The hub applies the search server-side (see `UsersView`), so the list is who actually
  // matches, not a client-side filter over whatever page happened to load first.
  const list = useResource(usersResource, { query: query.trim() || undefined, limit: 20 });
  const users = list.data?.users ?? [];

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button type="button" variant="outline" role="combobox" aria-expanded={open} className="w-full justify-between font-normal">
          <span className="min-w-0 truncate">{value ? value.email || value.userId : t("admin.alloc.grants.pickUser")}</span>
          <ChevronsUpDown className="size-4 shrink-0 opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-72 p-0" align="start">
        <Command search={query} onSearchChange={setQuery}>
          <CommandInput placeholder={t("admin.users.searchPlaceholder")} />
          <CommandList>
            {list.isLoading ? null : users.length === 0 ? (
              <CommandEmpty>{t("admin.alloc.grants.noUserMatch")}</CommandEmpty>
            ) : (
              <CommandGroup>
                {users.map((u) => (
                  <CommandItem
                    key={u.user_id}
                    value={u.user_id}
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
            )}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

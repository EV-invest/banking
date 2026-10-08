// Stands in for `@evinvest/uikit` in `user-picker.test.ts`: the real kit, with the picker
// already open and the operator's words already typed.
//
// The test renders with `react-dom/server` — the cabinet's suite has no DOM — so nothing can
// click the trigger or type into the field. Two parts are replaced to reach the state a
// browser reaches that way: the popover renders its content in place, and `Command` is
// handed the search the test typed. Everything that decides what the list shows — the kit's
// own filter in `Command`/`CommandItem` and its empty state — is the real kit code.

import * as kit from "@evinvest/uikit";
import { createElement, Fragment, type ComponentProps, type ReactNode } from "react";

export * from "@evinvest/uikit";

let typed = "";

/** What the operator has typed into the picker's search field. */
export function typeIntoPicker(query: string): void {
  typed = query;
}

/** Open from the start: the trigger and the content render in place, no portal. */
export function Popover({ children }: { children?: ReactNode }) {
  return createElement(Fragment, null, children);
}
export const PopoverTrigger = Popover;
export const PopoverContent = Popover;

export function Command(props: ComponentProps<typeof kit.Command>) {
  return createElement(kit.Command, { ...props, search: typed });
}

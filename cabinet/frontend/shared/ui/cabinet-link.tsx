"use client";

import NextLink from "next/link";
import { useParams } from "next/navigation";
import type { ComponentProps } from "react";

import { isLocale } from "@evinvest/i18n";

import { cabinetPath } from "@/shared/config/base-path";

/**
 * `next/link` for in-cabinet navigation, with the zone prefix applied.
 *
 * This exists because `basePath` is gone. It used to add `/cabinet` to every
 * href for free, but it also would have added it to the locale — turning
 * `/ru/cabinet/wallet` into `/cabinet/ru/cabinet/wallet` — so pages moved to a
 * `[locale]/cabinet` route tree and the prefix became this component's job.
 * (`docs/i18n-cabinet-routing-spike.md` has the measurements.)
 *
 * A bare `next/link` still compiles and still type-checks; it just points at the
 * conductor's origin root, which is a 404 on someone else's page. That is a
 * failure no tool in this repo catches, which is the argument for routing every
 * link through one component rather than 23 call sites remembering a rule.
 *
 * The locale comes from `useParams()` rather than a prop so a link does not have
 * to be handed something every caller would forward identically. English is the
 * floor: a link that renders in the wrong language is recoverable, a link that
 * throws while rendering a page is not.
 */
export function Link({
  href,
  ...props
}: Omit<ComponentProps<typeof NextLink>, "href"> & { href: `/${string}` }) {
  const params = useParams();
  const raw = params?.locale;
  const locale = isLocale(raw) ? raw : "en";
  return <NextLink href={cabinetPath(locale, href)} {...props} />;
}

// The words for a draft end, resolved through the caller's translator.

import type { Translate } from "@evinvest/i18n";

import { networkLabel } from "@/shared/lib/rail";
import { type EndDraft, type EndKind, needsId } from "@/views/admin/payments/lib/terms";

const endKindWords = (t: Translate): Record<EndKind, string> => ({
  service: t("admin.payments.kind.service", "A product"),
  user: t("admin.payments.kind.user", "An investor"),
  external: t("admin.payments.kind.external", "An external address"),
});

export function endKindLabel(kind: EndKind, t: Translate): string {
  return endKindWords(t)[kind];
}

/**
 * An end as the review sentence names it — the same words the picker showed, so what the
 * operator confirms is what they chose, not a wire id they never saw.
 */
export function draftWords(end: EndDraft, t: Translate): string {
  if (end.kind === "external") return `${networkLabel(end.network)} · ${end.address.trim()}`;
  if (needsId(end.kind)) return `${endKindLabel(end.kind, t)} · ${end.name || end.id}`;
  return endKindLabel(end.kind, t);
}

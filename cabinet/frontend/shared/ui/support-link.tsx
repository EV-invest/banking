"use client";

import { useT } from "@evinvest/i18n/react";

import { SUPPORT_EMAIL } from "@/shared/config/support";
import { cn } from "@/shared/lib/cn";

/**
 * The one way out of a sentence that ends in "contact support": a `mailto:` to the mailbox
 * the operators actually run (`@/shared/config/support`). A refusal that names support and
 * shows no address is the dead end #386 closes, so any copy that says "contact support"
 * renders this beside it rather than leaving the reader to guess where.
 *
 * The address is encoded, not interpolated: `?`/`&` in it would become the query half of
 * the `mailto:` — the same rule the KYC surfaces apply at their point of use.
 */
export function SupportLink({ className }: { className?: string }) {
  const t = useT();
  return (
    <a href={`mailto:${encodeURIComponent(SUPPORT_EMAIL)}`} className={cn("font-medium text-primary-ink underline underline-offset-2 outline-none focus-visible:ring-2 focus-visible:ring-ring", className)}>
      {t("profile.kyc.contact", { contact: SUPPORT_EMAIL })}
    </a>
  );
}

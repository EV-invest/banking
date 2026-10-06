"use client";

// Documents and disclosures, and the way to a person: one section for both breakpoints —
// the desktop rail renders it in place, the mobile stack pushes it as its own screen.
//
// Only what exists. The whitepaper is a page the site publishes, the source is on GitHub;
// "how funds are held" and the risk line are statements the cabinet can stand behind today
// — custody and identity facts, with the vendors named (Turnkey, Didit, Google: the owner
// allows naming client-facing services), nothing about reconciliation (#245's phase 1 is
// what would let it say more). The terms, privacy and risk-disclosure pages the issue
// lists are `undefined` in `SITE_DOCUMENTS` and so absent here, rather than links to `#`
// (#385).
//
// Built from the cabinet's `list-card` vocabulary rather than uikit's `Item*`, so the
// cards read as one system with the Settings and Profile screens beside them.

import { useLocale, useT } from "@evinvest/i18n/react";
import { ExternalLink, Mail } from "lucide-react";

import { SITE_DOCUMENTS, siteDocumentHref } from "@/shared/config/documents";
import { SUPPORT_EMAIL } from "@/shared/config/support";
import { cn } from "@/shared/lib/cn";
import { Chevron, Hairline, ListCard, ListCardTitle, ROW_INTERACTIVE, Row, RowLabel } from "@/shared/ui/list-card";

export function DocumentsSection() {
  const t = useT();
  const locale = useLocale();
  const { whitepaper, sourceCode } = SITE_DOCUMENTS;
  return (
    <div className="flex flex-col gap-4 lg:gap-4.5">
      <ListCard className="lg:px-5.5">
        <ListCardTitle>{t("settings.documents.published", "Published documents")}</ListCardTitle>
        <Hairline />
        {whitepaper !== undefined && (
          <>
            {/* The site's page, in the reader's locale. A plain anchor, not the cabinet Link:
                the destination is the conductor's, outside the zone — the same origin, so no
                new tab; the caption says where it leads. */}
            <a href={siteDocumentHref(locale, whitepaper)} className={ROW_INTERACTIVE}>
              <RowLabel title={t("settings.documents.whitepaper", "Whitepaper")} sub={t("settings.documents.whitepaperSub", "The fund's thesis and structure, on the public site")} />
              <ExternalLink className="size-4 shrink-0 text-ink-soft" aria-hidden />
            </a>
            <Hairline />
          </>
        )}
        {/* Another origin, so a new tab is acceptable here — and announced, since the icon
            alone says nothing to a screen reader. */}
        <a href={sourceCode} target="_blank" rel="noopener" className={ROW_INTERACTIVE}>
          <RowLabel title={t("settings.documents.sourceCode", "Open source")} sub={t("settings.documents.sourceCodeSub", "The fund's code, on GitHub")} />
          <span className="sr-only">{t("settings.documents.newTab", "opens in a new tab")}</span>
          <ExternalLink className="size-4 shrink-0 text-ink-soft" aria-hidden />
        </a>
      </ListCard>

      <ListCard className="lg:px-5.5">
        <ListCardTitle sub={t("settings.documents.custodySub", "Deposits are held in the fund's custody wallets at Turnkey on the network you sent them over, and booked to your identity as verified by Didit. Nothing is moved without that identity, and withdrawals go only to an address you request. You sign in with your Google account.")}>{t("settings.documents.custody", "How funds are held")}</ListCardTitle>
        <Hairline />
        <Row>
          <RowLabel title={t("settings.documents.risk", "Risk")} sub={t("auth.stat.risk", "Capital at risk. A target is not a forecast, and returns are not guaranteed.")} />
        </Row>
      </ListCard>

      <ListCard className="lg:px-5.5">
        <ListCardTitle sub={t("settings.support.sub", "A person reads every message")}>{t("nav.support", "Support")}</ListCardTitle>
        <Hairline />
        {/* The same mailbox every KYC dead end offers — one address, one place it is set. */}
        <a href={`mailto:${encodeURIComponent(SUPPORT_EMAIL)}`} className={ROW_INTERACTIVE}>
          <RowLabel title={t("settings.support.email", "Email support")} sub={SUPPORT_EMAIL} />
          <Mail className="size-4 shrink-0 text-ink-soft" aria-hidden />
        </a>
      </ListCard>
    </div>
  );
}

/** The mobile root-screen card into this section, with the mailbox one tap nearer: the
 *  tab bar has no Support slot, so below `lg` this row is where Support lives. */
export function MobileHelpCard({ onOpen }: { onOpen: () => void }) {
  const t = useT();
  return (
    <ListCard>
      <button type="button" onClick={onOpen} className={cn(ROW_INTERACTIVE, "w-full")}>
        <RowLabel title={t("settings.documents.title", "Documents and disclosures")} sub={t("settings.documents.rowSub", "Whitepaper, open source, how funds are held, risk")} />
        <Chevron />
      </button>
      <Hairline />
      <a href={`mailto:${encodeURIComponent(SUPPORT_EMAIL)}`} className={ROW_INTERACTIVE}>
        <RowLabel title={t("nav.support", "Support")} sub={SUPPORT_EMAIL} />
        <Mail className="size-4 shrink-0 text-ink-soft" aria-hidden />
      </a>
    </ListCard>
  );
}

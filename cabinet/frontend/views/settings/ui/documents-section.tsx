"use client";

// Documents and disclosures, and the way to a person: one section for both breakpoints —
// the desktop rail renders it in place, the mobile stack pushes it as its own screen.
//
// Only what exists. The whitepaper is a page the site publishes; "how funds are held" and
// the risk line are statements the cabinet can stand behind today — custody and identity
// facts, nothing about reconciliation (#245's phase 1 is what would let it say more). The
// terms, privacy and risk-disclosure pages the issue lists are `undefined` in
// `SITE_DOCUMENTS` and so absent here, rather than links to `#` (#385).

import { useLocale, useT } from "@evinvest/i18n/react";
import { ExternalLink, LifeBuoy } from "lucide-react";

import { SITE_DOCUMENTS, siteDocumentHref } from "@/shared/config/documents";
import { SUPPORT_EMAIL } from "@/shared/config/support";
import { cn } from "@/shared/lib/cn";
import { Chevron, Hairline, ListCard, ListCardTitle, Row, RowLabel } from "@/shared/ui/list-card";

// The tappable rows are hand-written anchors (uikit has no list-row control), so each
// carries its own focus ring — the same string the mobile settings cards use.
const ROW_LINK = "flex min-w-0 items-center justify-between gap-3 rounded-md py-3.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring";

export function DocumentsSection() {
  const t = useT();
  const locale = useLocale();
  const { whitepaper } = SITE_DOCUMENTS;
  return (
    <div className="flex flex-col gap-4 lg:gap-4.5">
      <ListCard className="lg:px-5.5">
        <ListCardTitle sub={t("settings.documents.sub")}>{t("settings.documents.title")}</ListCardTitle>
        <Hairline />
        {whitepaper !== undefined && (
          <>
            {/* The site's page, in the reader's locale. A plain anchor, not the cabinet
                Link: the destination is the conductor's, outside the zone. */}
            <a href={siteDocumentHref(locale, whitepaper)} target="_blank" rel="noopener" className={ROW_LINK}>
              <RowLabel title={t("settings.documents.whitepaper")} sub={t("settings.documents.whitepaperSub")} />
              <ExternalLink className="size-4 shrink-0 text-ink-soft" aria-hidden />
            </a>
            <Hairline />
          </>
        )}
        <Row>
          <RowLabel title={t("settings.documents.custody")} sub={t("settings.documents.custodySub")} />
        </Row>
        <Hairline />
        <Row>
          <RowLabel title={t("settings.documents.risk")} sub={t("auth.stat.risk")} />
        </Row>
      </ListCard>

      <ListCard className="lg:px-5.5">
        <ListCardTitle sub={t("settings.support.sub")}>{t("nav.support")}</ListCardTitle>
        <Hairline />
        {/* The same mailbox every KYC dead end offers — one address, one place it is set. */}
        <a href={`mailto:${encodeURIComponent(SUPPORT_EMAIL)}`} className={cn(ROW_LINK, "gap-3")}>
          <LifeBuoy className="size-4.5 shrink-0 text-primary-ink" aria-hidden />
          <RowLabel title={t("settings.support.email")} sub={SUPPORT_EMAIL} />
          <Chevron />
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
      <button type="button" onClick={onOpen} className={cn(ROW_LINK, "w-full")}>
        <RowLabel title={t("settings.documents.title")} sub={t("settings.documents.rowSub")} />
        <Chevron />
      </button>
      <Hairline />
      <a href={`mailto:${encodeURIComponent(SUPPORT_EMAIL)}`} className={ROW_LINK}>
        <RowLabel title={t("nav.support")} sub={SUPPORT_EMAIL} />
        <Chevron />
      </a>
    </ListCard>
  );
}

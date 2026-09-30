"use client";

import { ArrowUpRight } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Button, Card } from "@evinvest/uikit";

import { config } from "@/config";
import { usePanelViewer } from "@/features/panel-access";
import { cn } from "@/shared/lib/cn";
import { Link } from "@/shared/ui/cabinet-link";
import { CARD_PAD } from "@/views/dashboard/lib/chrome";
import { SERVICE_ARB, servicePanelLink } from "@/views/dashboard/lib/service-panel";

// The way into the Service-Arb panel for the people who work it. A name and a link, nothing
// else: the panel is its own origin and says what it is once opened. Same tab — it signs in
// through the same identity plane, so there is no session to keep this one for.
export function ServicePanelCard({ className }: { className?: string }) {
  const t = useT();
  const viewer = usePanelViewer();
  const link = viewer.ready ? servicePanelLink(config.public.saPanelUrl, viewer.role, viewer.scopes) : null;
  if (!link) return null;

  return (
    <Card className={cn("flex-row flex-wrap items-center justify-between gap-3 py-4", CARD_PAD, className)}>
      <a href={link.href} className="group inline-flex items-center gap-2 whitespace-nowrap rounded-md font-semibold text-ink outline-none focus-visible:ring-2 focus-visible:ring-ring">
        {t("dash.servicePanel")}
        <ArrowUpRight className="size-4 text-ink-soft transition-colors group-hover:text-ink" aria-hidden />
      </a>
      {link.manage && (
        <Button asChild variant="outline" size="sm">
          <Link href={`/admin/allocations/${SERVICE_ARB}`}>{t("panelAccess.action")}</Link>
        </Button>
      )}
    </Card>
  );
}

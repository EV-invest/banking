"use client";

import { useT } from "@evinvest/i18n/react";
import { Skeleton } from "@evinvest/uikit";

import { canManagePanelAccess } from "@/entities/scope/lib/access";
import { PanelAccessCard, usePanelViewer } from "@/features/panel-access";
import { ForbiddenScreen } from "@/shared/ui/forbidden-screen";
import { Settled, StaggerItem } from "@/shared/ui/motion";
import { AdminHeader, AdminScreen } from "@/views/admin/ui/shell";

// The allocation's panel access and nothing else: the other registry panels read money
// data a scope's admin has no right to, so they are not offered here even to a console
// admin, who has them one click away in the registry.
export function PanelAccessView({ service }: { service: string }) {
  const t = useT();
  const viewer = usePanelViewer();
  const allowed = canManagePanelAccess(viewer.role, viewer.scopes, service);

  if (viewer.ready && !allowed) return <ForbiddenScreen />;

  return (
    <AdminScreen className="space-y-8">
      <AdminHeader eyebrow={t("admin.eyebrow.administer")} title={t("panelAccess.action")} subtitle={service} />
      <StaggerItem as="section" className="max-w-xl">
        <Settled loading={!viewer.ready} skeleton={<Skeleton className="h-64 w-full" />}>
          <PanelAccessCard service={service} className="w-full" />
        </Settled>
      </StaggerItem>
    </AdminScreen>
  );
}

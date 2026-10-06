"use client";

import { useT } from "@evinvest/i18n/react";

import { PanelAccessCard } from "@/features/panel-access";
import { StaggerItem } from "@/shared/ui/motion";
import { AdminHeader, AdminScreen } from "@/views/admin/ui/shell";

// The allocation's panel access and nothing else: the other registry panels read money
// data a delegate has no right to, so they are not offered here even to a console admin,
// who has them one click away in the registry. Who may manage it is the roster read's to
// say, so a refusal surfaces inside the card, worded for the list.
export function PanelAccessView({ service, namespace }: { service: string; namespace: string }) {
  const t = useT();
  return (
    <AdminScreen className="space-y-8">
      <AdminHeader eyebrow={t("admin.eyebrow.administer", "Administer")} title={t("panelAccess.action", "Panel access")} subtitle={service} />
      <StaggerItem as="section" className="max-w-xl">
        <PanelAccessCard namespace={namespace} className="w-full" />
      </StaggerItem>
    </AdminScreen>
  );
}

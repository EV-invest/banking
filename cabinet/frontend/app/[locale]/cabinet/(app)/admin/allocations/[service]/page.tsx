import { notFound } from "next/navigation";
import { I18nScope } from "@evinvest/i18n/react";

import { isServiceId, tenantOf } from "@/entities/grant/lib/access";
import { PanelAccessView } from "@/views/admin/panel-access/ui/panel-access-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// One allocation's panel access, on a page of its own so a delegate — who holds no console
// role and cannot read the registry — has somewhere to manage it. Authorized by the
// identity plane on every call.
export default async function AllocationPanelAccessPage({ params }: { params: Promise<{ service: string }> }) {
  // Not decoded: a valid id has nothing to decode, and one that needs it is not valid.
  const { service } = await params;
  const namespace = isServiceId(service) ? tenantOf(service) : null;
  if (namespace === null) notFound();
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/admin/allocations/[service]/page.tsx")}>
      <PanelAccessView service={service} namespace={namespace} />
    </I18nScope>
  );
}

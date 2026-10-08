import { I18nScope } from "@evinvest/i18n/react";

import { AllocationsView } from "@/views/admin/allocations/ui/allocations-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// Admin console — the allocation registry (register a product, open/close it). Authorized server-side.
export default async function AdminAllocationsPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/admin/allocations/page.tsx")}>
      <AllocationsView />
    </I18nScope>
  );
}

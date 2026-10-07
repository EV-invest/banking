import { I18nScope } from "@evinvest/i18n/react";

import { OperationsView } from "@/views/operations/ui/operations-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

export default async function OperationsPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/operations/page.tsx")}>
      <OperationsView />
    </I18nScope>
  );
}

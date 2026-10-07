import { I18nScope } from "@evinvest/i18n/react";

import { DashboardView } from "@/views/dashboard/ui/dashboard-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// The cabinet home — the investor's portfolio dashboard (Figma `cabinet/home`). Data is
// fetched client-side through the BFF, which authorizes each hub call with the session's
// access token.
export default async function Page() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/page.tsx")}>
      <DashboardView />
    </I18nScope>
  );
}

import { I18nScope } from "@evinvest/i18n/react";

import { InvestView } from "@/views/invest/ui/invest-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// The fund-shares surface: the portfolio summary and one row per product. Dealing moved
// to `/invest/[service]` — a subscription is a decision about one fund, and a form under
// every row made the list read as a stack of forms rather than as a portfolio.
export default async function InvestPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/invest/page.tsx")}>
      <InvestView />
    </I18nScope>
  );
}

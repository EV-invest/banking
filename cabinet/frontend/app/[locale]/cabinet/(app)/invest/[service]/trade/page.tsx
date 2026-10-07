import { I18nScope } from "@evinvest/i18n/react";

import { TradeView } from "@/views/trade/ui/trade-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// The terminal for one product's book, keyed by the same `service` slug as the product
// page above it. Thin on purpose: the page is the route, the view is the screen.
export default async function TradePage({ params }: { params: Promise<{ service: string }> }) {
  const { service } = await params;
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/invest/[service]/trade/page.tsx")}>
      <TradeView service={decodeURIComponent(service)} />
    </I18nScope>
  );
}

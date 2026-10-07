import { I18nScope } from "@evinvest/i18n/react";

import { WalletOverviewView } from "@/views/wallet/ui/wallet-overview-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// The investor wallet surface — one balance and the rails that move money in and out.
// Deposit, withdraw and activity are their own routes (Figma `cabinet/wallet/*`). Data is
// fetched client-side through the BFF, which authorizes each hub call with the session's
// access token; an unauthenticated visitor sees the load error.
export default async function WalletPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/wallet/page.tsx")}>
      <WalletOverviewView />
    </I18nScope>
  );
}

import { I18nScope } from "@evinvest/i18n/react";

import { WithdrawView } from "@/views/wallet/ui/withdraw-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// Send USDT to an external address. `?network=` preselects the rail (see the deposit route
// for why it's read server-side).
export default async function WalletWithdrawPage({ searchParams }: { searchParams: Promise<{ network?: string }> }) {
  const { network } = await searchParams;
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/wallet/withdraw/page.tsx")}>
      <WithdrawView initialNetwork={network} />
    </I18nScope>
  );
}

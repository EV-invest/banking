import { I18nScope } from "@evinvest/i18n/react";

import { WithdrawalsView } from "@/views/admin/withdrawals/ui/withdrawals-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// Admin console — the withdrawal operator queue (dispatch / settle / fail). Authorized server-side.
export default async function AdminWithdrawalsPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/admin/withdrawals/page.tsx")}>
      <WithdrawalsView />
    </I18nScope>
  );
}

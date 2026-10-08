import { I18nScope } from "@evinvest/i18n/react";

import { PaymentsView } from "@/views/admin/payments/ui/payments-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// Admin console — payment orders between the platform's claims and out to a chain.
// Authorized server-side; the BFF re-checks `PaymentOpen` (Admin/Owner) at the money plane.
export default async function AdminPaymentsPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/admin/payments/page.tsx")}>
      <PaymentsView />
    </I18nScope>
  );
}

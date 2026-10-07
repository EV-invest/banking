import { I18nScope } from "@evinvest/i18n/react";

import { UsersView } from "@/views/admin/users/ui/users-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// Admin console — users (identities, KYC, roles, sessions). Authorized server-side.
export default async function AdminUsersPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/admin/users/page.tsx")}>
      <UsersView />
    </I18nScope>
  );
}

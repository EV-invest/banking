import { I18nScope } from "@evinvest/i18n/react";

import { ProfileView } from "@/views/profile/ui/profile-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// The investor profile surface; identity is fetched client-side via the BFF session.
export default async function ProfilePage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/profile/page.tsx")}>
      <ProfileView />
    </I18nScope>
  );
}

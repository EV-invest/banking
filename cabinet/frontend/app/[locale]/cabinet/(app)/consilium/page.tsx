import { I18nScope } from "@evinvest/i18n/react";

import { ConsiliumView } from "@/views/consilium/ui/consilium-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// The owners' room. Gated by the session like every other `(app)` page; the BFF gates it
// again on the caller actually being an owner, and answers 403 to anyone who is not.
export default async function ConsiliumPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/consilium/page.tsx")}>
      <ConsiliumView />
    </I18nScope>
  );
}

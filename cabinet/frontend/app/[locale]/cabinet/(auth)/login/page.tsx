import { I18nScope } from "@evinvest/i18n/react";

import { LoginView, type LoginSearchParams } from "@/views/login/ui/login";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

export default async function Page({ searchParams }: { searchParams: Promise<LoginSearchParams> }) {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(auth)/login/page.tsx")}>
      <LoginView searchParams={searchParams} />
    </I18nScope>
  );
}

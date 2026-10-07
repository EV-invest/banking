import { I18nScope } from "@evinvest/i18n/react";

import { sectionFrom } from "@/views/settings/lib/sections";
import { SettingsView } from "@/views/settings/ui/settings-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// The investor settings surface; identity is fetched client-side via the BFF session.
// `?section=` opens a section directly — the profile page sends the reader to
// `?section=personal`, the Access card to `sessions` — and is read here, server side,
// rather than with `useSearchParams`, so the view needs no Suspense boundary.
export default async function SettingsPage({ searchParams }: { searchParams: Promise<{ section?: string }> }) {
  const { section } = await searchParams;
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/settings/page.tsx")}>
      <SettingsView initialSection={sectionFrom(section)} />
    </I18nScope>
  );
}

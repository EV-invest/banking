import { Suspense } from "react";

import { I18nScope } from "@evinvest/i18n/react";

import { readNotificationSettings } from "@/entities/notification/api/notification-server";
import { readProfile } from "@/entities/user/api/profile-server";
import { isClientNavigation } from "@/shared/api/server/navigation";
import { type Section, sectionFrom } from "@/views/settings/lib/sections";
import { type SettingsSeeds, SettingsView } from "@/views/settings/ui/settings-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// The investor settings surface. `?section=` opens a section directly — the profile page
// sends the reader to `?section=personal`, the Access card to `sessions` — and is read
// here, server side, rather than with `useSearchParams`, so the view needs no Suspense
// boundary of its own.
//
// The form's data is read on the server too — the /wallet pattern: the boundary's fallback
// is the same view in its loading state, flushed at once, and the reads stream in behind
// it. The delivery preferences are read only for the section that shows them. A failed or
// timed-out read arrives as no seed and that part makes the browser read it always made;
// everything interactive stays in the browser.
export default async function SettingsPage({ searchParams }: { searchParams: Promise<{ section?: string }> }) {
  const { section } = await searchParams;
  const open = sectionFrom(section);
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/settings/page.tsx")}>
      <Suspense fallback={<SettingsView initialSection={open} />}>
        <SeededSettings section={open} />
      </Suspense>
    </I18nScope>
  );
}

async function SeededSettings({ section }: { section: Section }) {
  // Every rail click is a `router.replace` back to this page; re-reading the profile for
  // each one would be a BFF request per tab, racing a save made just before it.
  if (await isClientNavigation()) return <SettingsView initialSection={section} />;
  const [profile, notificationSettings] = await Promise.all([readProfile(), section === "notifications" ? readNotificationSettings() : null]);
  const seeds: SettingsSeeds = {
    ...(profile && { profile }),
    ...(notificationSettings && { notificationSettings }),
  };
  return <SettingsView initialSection={section} initial={Object.keys(seeds).length > 0 ? seeds : undefined} />;
}

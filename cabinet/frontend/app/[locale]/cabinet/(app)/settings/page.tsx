import { sectionFrom } from "@/views/settings/lib/sections";
import { SettingsView } from "@/views/settings/ui/settings-view";

// The investor settings surface; identity is fetched client-side via the BFF session.
// `?section=` opens a section directly — the profile page sends the reader to
// `?section=personal`, the Access card to `sessions` — and is read here, server side,
// rather than with `useSearchParams`, so the view needs no Suspense boundary.
export default async function SettingsPage({ searchParams }: { searchParams: Promise<{ section?: string }> }) {
  const { section } = await searchParams;
  return <SettingsView initialSection={sectionFrom(section)} />;
}

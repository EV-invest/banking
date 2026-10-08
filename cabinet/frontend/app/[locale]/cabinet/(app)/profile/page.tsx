import { Suspense } from "react";

import { I18nScope } from "@evinvest/i18n/react";

import { readPositions } from "@/entities/fund/api/fund-server";
import { readOperations } from "@/entities/operation/api/operation-server";
import { readProfile } from "@/entities/user/api/profile-server";
import { readWallet } from "@/entities/wallet/api/wallet-server";
import { isClientNavigation } from "@/shared/api/server/navigation";
import { type ProfileSeeds, ProfileView } from "@/views/profile/ui/profile-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// The investor profile, read on the server — the /wallet pattern: the boundary's fallback
// is the same view in its loading state, flushed at once, and the four BFF reads stream in
// behind it together. Each read that fails or times out is simply missing from the seeds,
// and that card makes the browser read it always made. The session count is not among
// them: it is the shell's endpoint, not the BFF's. A client-side navigation skips the
// reads: the browser cache answers those.
export default async function ProfilePage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/profile/page.tsx")}>
      <Suspense fallback={<ProfileView />}>
        <SeededProfile />
      </Suspense>
    </I18nScope>
  );
}

async function SeededProfile() {
  if (await isClientNavigation()) return <ProfileView />;
  const [profile, positions, wallet, operations] = await Promise.all([readProfile(), readPositions(), readWallet(), readOperations()]);
  const seeds: ProfileSeeds = {
    ...(profile && { profile }),
    ...(positions && { positions }),
    ...(wallet && { wallet }),
    ...(operations && { operations }),
  };
  return <ProfileView initial={Object.keys(seeds).length > 0 ? seeds : undefined} />;
}

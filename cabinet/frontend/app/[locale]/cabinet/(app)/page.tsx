import { cookies } from "next/headers";
import { Suspense } from "react";

import { I18nScope } from "@evinvest/i18n/react";

import { readAllocations, readPositions } from "@/entities/fund/api/fund-server";
import { readOperations } from "@/entities/operation/api/operation-server";
import { RECENT_OPS } from "@/entities/operation/lib/recent";
import { readWallet } from "@/entities/wallet/api/wallet-server";
import { type ChecklistShape, isChecklistShape, SHAPE_COOKIE } from "@/features/onboarding";
import { DashboardView } from "@/views/dashboard/ui/dashboard-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// The cabinet home — the investor's portfolio dashboard (Figma `cabinet/home`).
//
// The four reads behind its figures are made on the server, the way /wallet makes its one,
// and the page never waits for them: the boundary's fallback is the same dashboard in its
// loading state, flushed at once, and the reads stream in behind it. Each read that fails
// or times out arrives as no seed, and its card makes the browser read it always made. The
// verification checklist stays a browser read: `/kyc/status` is the identity plane's route,
// not the BFF's, so the server has no credential that reaches it — it takes the shape this
// browser last saw it in (a cookie the block writes itself) so neither path moves the page.
export default async function Page() {
  const locale = await currentLocale();
  const shape = (await cookies()).get(SHAPE_COOKIE)?.value;
  const hint: ChecklistShape | undefined = isChecklistShape(shape) ? shape : undefined;
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/page.tsx")}>
      <Suspense fallback={<DashboardView checklistHint={hint} />}>
        <SeededDashboard hint={hint} />
      </Suspense>
    </I18nScope>
  );
}

async function SeededDashboard({ hint }: { hint: ChecklistShape | undefined }) {
  const [wallet, positions, operations, catalog] = await Promise.all([readWallet(), readPositions(), readOperations(RECENT_OPS), readAllocations()]);
  return <DashboardView checklistHint={hint} initial={{ wallet: wallet ?? undefined, positions: positions ?? undefined, operations: operations ?? undefined, catalog: catalog ?? undefined }} />;
}

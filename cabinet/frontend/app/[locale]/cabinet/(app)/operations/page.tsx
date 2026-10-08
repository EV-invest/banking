import { Suspense } from "react";

import { I18nScope } from "@evinvest/i18n/react";

import { readAllocations } from "@/entities/fund/api/fund-server";
import { readOperations } from "@/entities/operation/api/operation-server";
import { OperationsView } from "@/views/operations/ui/operations-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// The activity timeline, read on the server so the first HTML carries the rows — the
// /wallet pattern: the boundary's fallback is the same view in its loading state, flushed
// at once, and the reads stream in behind it. A failed or timed-out read arrives as no
// seed and the view makes the browser read it always made. The catalog rides along only
// to name the funds on that first paint; without it the rows show slugs until it lands.
export default async function OperationsPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/operations/page.tsx")}>
      <Suspense fallback={<OperationsView />}>
        <SeededOperations />
      </Suspense>
    </I18nScope>
  );
}

async function SeededOperations() {
  const [operations, catalog] = await Promise.all([readOperations(), readAllocations()]);
  return <OperationsView initial={operations ?? undefined} catalog={catalog ?? undefined} />;
}

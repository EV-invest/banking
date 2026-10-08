import { cookies } from "next/headers";

import { I18nScope } from "@evinvest/i18n/react";

import { type ChecklistShape, isChecklistShape, SHAPE_COOKIE } from "@/features/onboarding";
import { DashboardView } from "@/views/dashboard/ui/dashboard-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// The cabinet home — the investor's portfolio dashboard (Figma `cabinet/home`). Data is
// fetched client-side through the BFF, which authorizes each hub call with the session's
// access token.
//
// The onboarding block's placeholder takes the shape this browser last saw it in (a cookie
// the block writes itself — `features/onboarding/lib/checklist-shape`), so the server's HTML
// holds the right amount of room for it before any read has answered.
export default async function Page() {
  const locale = await currentLocale();
  const shape = (await cookies()).get(SHAPE_COOKIE)?.value;
  const hint: ChecklistShape | undefined = isChecklistShape(shape) ? shape : undefined;
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/page.tsx")}>
      <DashboardView checklistHint={hint} />
    </I18nScope>
  );
}

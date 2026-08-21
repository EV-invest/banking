import { Forbidden } from "@evinvest/uikit";

// Next's `forbidden.tsx` convention (403) — rendered when the `forbidden()`
// interrupt fires from a Server Component / Route Handler. The shared surface
// from @evinvest/uikit; "back to home" returns to the cabinet dashboard.
export default function ForbiddenPage() {
  return <Forbidden homeHref="/cabinet" />;
}

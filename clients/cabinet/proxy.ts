import { createAbMiddleware } from "@evinvest/experiments/next";

import { experiments } from "@/application/experiments";

// A/B assignment boundary (Next 16 "proxy", formerly middleware; Node runtime).
// Assigns a sticky `ab_<key>` cookie per experiment in the registry on first
// visit. A no-op while `experiments` is empty.
export const proxy = createAbMiddleware(experiments);

export const config = {
  matcher: ["/((?!api|_next/static|_next/image|favicon.ico).*)"],
};

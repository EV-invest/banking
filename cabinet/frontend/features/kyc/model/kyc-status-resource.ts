"use client";

// The caller's verification state, as the identity plane holds it: their tier, and the
// attempt that is running right now if there is one.
//
// This is the resource that closes #190. Before it, the only input to "offer Start?" was
// `kyc_level`, which is precisely the value that lags while a case is running — so a user
// coming back from the vendor before the webhook landed was shown the same Start button they
// had just pressed, and a second paid vendor session was one click away.
//
// It also takes over the polling `entities/user/model/profile-resource` used to do (#219).
// That loop watched `kyc_level` move off 0 as a stand-in for "the verdict arrived"; the
// verdict has a name now, so the loop watches the case instead and refreshes the profile once
// something actually moved.

import { fetchKycStatus } from "@/features/kyc/api/kyc-client";
import { defineResource } from "@/shared/lib/resource";

export const kycStatusResource = defineResource({
  name: "kyc.status",
  fetch: fetchKycStatus,
  // Short: a verdict landing while the page sits open changes what the screen may offer, and
  // a stale "no case" is the one that costs money.
  revalidate: 15,
  poll: {
    // Three states are worth another look, and a verified caller with nothing running is not
    // one of them:
    //   · a running case — the verdict can land at any moment, and nothing else announces it;
    //   · tier 0 with no case — an operator can still raise the tier by hand, which is the
    //     path this cabinet is actually on today (see `@/shared/config/support`);
    //   · `null` — the plane could not be read. That is the pre-release window against a
    //     concierge without `/kyc/status`, and re-asking is what lets a tab that was open
    //     across the deployment pick the route up without a reload.
    while: (status) => status === null || status?.case != null || status?.level === 0,
    startMs: 5_000,
    maxMs: 60_000,
  },
});

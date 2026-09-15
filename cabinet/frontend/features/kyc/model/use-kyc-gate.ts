"use client";

// The money screens' half of `./use-kyc-status`: one answer to "does this surface stand
// closed?", on the authoritative source.
//
// The wallet used to decide this from `profile.kyc_level` alone. That field is MIRRORED from
// the identity plane across the bridge (`entities/user/model/profile-resource`), so it lands
// a poll behind — up to a minute, backing off from 5s. A reader coming back from the vendor
// therefore watched the banner disappear and the profile card stop offering a start, while
// /wallet went on showing "verify to use these networks" in place of rails the hub was
// already ready to serve. `/kyc/status` is the value that moved first; this reads it.
//
// `rails.length` is not an alternative signal: the hub lists a network for an unverified
// caller too, just with no address (`piggybank/core/tests/kyc_gating.rs`).

import { profileResource } from "@/entities/user/model/profile-resource";
import { isMoneyGated } from "@/features/kyc/lib/money-gate";
import { useKycStatus } from "@/features/kyc/model/use-kyc-status";
import { useResource } from "@/shared/lib/resource";

export interface MoneyGate {
  /** Show the verification block instead of the surface. */
  gated: boolean;
  /** Neither source has answered yet — show a skeleton, not a verdict. */
  loading: boolean;
}

export function useKycGate(): MoneyGate {
  const { level, known, loading } = useKycStatus();
  const { data: profile } = useResource(profileResource);

  // "Settled" is deliberately not "not loading": both reads can finish by failing, and a tier
  // nobody could read is not a tier of 0. `kycLevel` is what turns an absent profile into one.
  const settled = known || profile != null;
  return { gated: isMoneyGated({ level, settled, loading }), loading };
}

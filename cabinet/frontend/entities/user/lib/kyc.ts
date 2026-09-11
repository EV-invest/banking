import type { UserProfile } from "@/shared/contracts";

/**
 * The one line that decides both what the profile offers and what the money screens show.
 *
 * Only the entry tier is self-serve: `/kyc/start` opens a case for tier 1 and nothing above
 * it, so offering verification to someone already past 1 would spend a paid vendor session
 * on a verdict that, by the plane's own rule, cannot raise them. Higher tiers stay a
 * compliance action. The hub gates deposit addresses and withdrawals on the same tier, so
 * "may start verification" and "is refused a deposit address" are the same predicate — kept
 * here, in the entity that owns the field, rather than copied into each screen.
 *
 * An absent `kyc_level` counts as 0 rather than unknown — proto3 omits zero-valued scalars
 * from JSON, so "no field" is exactly how an unverified user arrives. An absent PROFILE is
 * the different case and answers `false`: nothing is known about that caller's tier yet, and
 * a screen that guessed would hide a deposit address from someone entitled to it.
 */
export function isUnverified(profile: UserProfile | null | undefined): boolean {
  return profile != null && (profile.kyc_level ?? 0) === 0;
}

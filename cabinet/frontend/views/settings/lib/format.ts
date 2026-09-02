// The settings surface's formatting entry point.
//
// The name/initials derivation moved to `@/shared/lib/identity` when the owners' roster
// began showing people too — see that module for why. Re-exported here so the settings,
// profile and account-chip call sites are unchanged.
export { displayName, initialsOf, initialsOfName, truncateName } from "@/shared/lib/identity";

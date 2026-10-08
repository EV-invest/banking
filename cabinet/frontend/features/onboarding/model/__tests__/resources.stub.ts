// Stand-in for the three entity resources `use-checklist.ts` reads (`fund-resource`,
// `profile-resource`, `wallet-resource`) in `use-checklist.test.ts`. The real ones pull the
// API client, which Node's type stripping cannot load; the hook only hands them to
// `useResource`, so a token per resource is all it needs.
export const positionsResource = { name: "positions" };
export const profileResource = { name: "profile" };
export const walletResource = { name: "wallet" };

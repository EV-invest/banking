// Display helpers for the portfolio dashboard. Money formatting lives in one module for
// the whole cabinet (`@/shared/lib/money`) — the dashboard's figures are summaries, so
// they take the 2-dp summary precision.

export { formatPct, formatSignedUsd, formatUsd, num, shortAddress } from "@/shared/lib/money";

// The dashboard's activity lines have far less room than the wallet's, so its addresses
// cut harder than the shared default.
export const DASH_ADDRESS = { head: 6, tail: 4, min: 16 } as const;

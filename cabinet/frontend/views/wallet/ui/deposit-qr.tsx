"use client";

import { Skeleton } from "@evinvest/uikit";
import dynamic from "next/dynamic";

// The QR encoder is only needed once an address is on screen, so it is fetched on demand
// rather than shipped with the route. While it arrives the slot keeps the address-loading
// skeleton's exact box, so the hand-off is skeleton → QR with nothing in between.
const loadPlate = () => import("@/views/wallet/ui/deposit-qr-plate");

export const DepositQr = dynamic(() => loadPlate().then((m) => m.DepositQr), {
  loading: () => <Skeleton className="size-40 shrink-0 rounded-xl lg:size-45 lg:rounded-2xl" />,
});

/** Starts fetching the encoder while the address is still loading; safe to call repeatedly. */
export function preloadDepositQr(): void {
  // Swallowed here only: the lazy component's own load retries and surfaces a failure.
  loadPlate().catch(() => undefined);
}

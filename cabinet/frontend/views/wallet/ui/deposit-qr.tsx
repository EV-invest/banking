"use client";

import { useT } from "@evinvest/i18n/react";
import { Skeleton } from "@evinvest/uikit";
import { QrCode } from "lucide-react";
import { type ComponentType, lazy, Suspense } from "react";

import { reportChunkError } from "@/shared/lib/chunk-error";

type QrProps = { value: string };

// The QR encoder is only needed once an address is on screen, so it is fetched on demand
// rather than shipped with the route.
const loadPlate = () => import("@/views/wallet/ui/deposit-qr-plate");

// A chunk that never arrives must not take the screen down with it: the address and its
// copy button are the deposit, the QR is a convenience. A failed download resolves to a
// neutral slot of the same box instead of rejecting into the route's error boundary.
const LazyPlate = lazy<ComponentType<QrProps>>(() =>
  loadPlate().then(
    (m) => ({ default: m.DepositQr }),
    (error: unknown) => {
      reportChunkError(error, "wallet: deposit QR encoder");
      return { default: QrUnavailable };
    },
  ),
);

// The slot keeps the address-loading skeleton's exact box while the encoder arrives, so
// the hand-off is skeleton → QR with nothing in between.
export function DepositQr({ value }: QrProps) {
  return (
    <Suspense fallback={<Skeleton className="size-40 shrink-0 rounded-xl lg:size-45 lg:rounded-2xl" />}>
      <LazyPlate value={value} />
    </Suspense>
  );
}

function QrUnavailable() {
  const t = useT();
  return (
    <div className="flex size-40 shrink-0 flex-col items-center justify-center gap-2 rounded-xl border border-border px-4 text-center lg:size-45 lg:rounded-2xl">
      <QrCode className="size-6 text-ink-soft" aria-hidden />
      <p className="text-xs text-ink-soft">{t("wallet.qrUnavailable", "QR code unavailable — use the address below.")}</p>
    </div>
  );
}

/** Starts fetching the encoder while the address is still loading; safe to call repeatedly. */
export function preloadDepositQr(): void {
  // Swallowed here only: the lazy component's own load reports a failure.
  loadPlate().catch(() => undefined);
}

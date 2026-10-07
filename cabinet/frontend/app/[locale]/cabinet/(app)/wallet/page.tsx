import { Suspense } from "react";

import { readWallet } from "@/entities/wallet/api/wallet-server";
import { WalletOverviewView } from "@/views/wallet/ui/wallet-overview-view";

// The investor wallet surface — one balance and the rails that move money in and out.
// Deposit, withdraw and activity are their own routes (Figma `cabinet/wallet/*`).
//
// The balance is read on the server so the first HTML carries the figures, but the page
// never waits for it: the boundary's fallback is the same view in its loading state (title,
// actions and skeletons — what this route always painted first), flushed at once, and the
// read streams in behind it. A read that fails or times out arrives as no seed, and the
// view makes the browser read it always made.
export default function WalletPage() {
  return (
    <Suspense fallback={<WalletOverviewView />}>
      <SeededWalletOverview />
    </Suspense>
  );
}

async function SeededWalletOverview() {
  const initial = (await readWallet()) ?? undefined;
  return <WalletOverviewView initial={initial} />;
}

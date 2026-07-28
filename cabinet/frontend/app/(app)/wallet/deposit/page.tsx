import { DepositView } from "@/views/wallet/ui/deposit-view";

// Top up the balance with crypto. `?network=` preselects a rail so the overview's per-rail
// Deposit button lands on the right one; it's read here (server side) rather than with
// `useSearchParams` so the view needs no Suspense boundary.
export default async function WalletDepositPage({ searchParams }: { searchParams: Promise<{ network?: string }> }) {
  const { network } = await searchParams;
  return <DepositView initialNetwork={network} />;
}

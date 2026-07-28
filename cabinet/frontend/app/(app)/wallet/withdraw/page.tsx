import { WithdrawView } from "@/views/wallet/ui/withdraw-view";

// Send USDT to an external address. `?network=` preselects the rail (see the deposit route
// for why it's read server-side).
export default async function WalletWithdrawPage({ searchParams }: { searchParams: Promise<{ network?: string }> }) {
  const { network } = await searchParams;
  return <WithdrawView initialNetwork={network} />;
}

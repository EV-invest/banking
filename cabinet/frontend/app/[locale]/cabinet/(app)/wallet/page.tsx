import { WalletOverviewView } from "@/views/wallet/ui/wallet-overview-view";

// The investor wallet surface — one balance and the rails that move money in and out.
// Deposit, withdraw and activity are their own routes (Figma `cabinet/wallet/*`). Data is
// fetched client-side through the BFF, which authorizes each hub call with the session's
// access token; an unauthenticated visitor sees the load error.
export default function WalletPage() {
  return <WalletOverviewView />;
}

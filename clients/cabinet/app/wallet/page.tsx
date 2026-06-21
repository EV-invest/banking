import { WalletView } from "@/views/wallet/ui/wallet-view";

// The investor wallet surface (balances · deposit · withdraw · activity). Data is
// fetched client-side through the BFF, which authorizes each hub call with the
// session's access token; an unauthenticated visitor sees the load error.
export default function WalletPage() {
  return <WalletView />;
}

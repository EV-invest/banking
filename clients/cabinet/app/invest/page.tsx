import { InvestView } from "@/views/invest/ui/invest-view";

// The investor fund-shares surface (positions · subscribe · redeem · activity). Data is
// fetched client-side through the BFF, which authorizes each hub call with the session's
// access token; an unauthenticated visitor sees the load error.
export default function InvestPage() {
  return <InvestView />;
}

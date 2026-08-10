import { InvestView } from "@/views/invest/ui/invest-view";

// The fund-shares surface: positions · subscribe · redeem. Unparked now that the screen
// picks its fund from the allocation registry instead of a hardcoded slug — the reason
// the placeholder went up was that this form could not name a real product.
export default function InvestPage() {
  return <InvestView />;
}

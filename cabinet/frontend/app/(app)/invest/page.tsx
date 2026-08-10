import { InvestView } from "@/views/invest/ui/invest-view";

// The fund-shares surface: the portfolio summary and one row per product. Dealing moved
// to `/invest/[service]` — a subscription is a decision about one fund, and a form under
// every row made the list read as a stack of forms rather than as a portfolio.
export default function InvestPage() {
  return <InvestView />;
}

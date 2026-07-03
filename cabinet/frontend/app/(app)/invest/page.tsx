import { LineChart } from "lucide-react";

import { PagePlaceholder } from "@/application/layout/page-placeholder";

// The fund-shares surface (positions · subscribe · redeem) is parked while the screen is
// redesigned; the implementation lives in views/invest/ui/invest-view.tsx for later. The
// placeholder keeps the route an honest nav destination in the meantime.
export default function InvestPage() {
  return (
    <PagePlaceholder
      eyebrow="Invest"
      title="Invest"
      blurb="Fund subscriptions and redemptions are getting their redesigned home — coming soon. Your existing positions stay visible on the dashboard."
      icon={<LineChart className="size-6" />}
    />
  );
}

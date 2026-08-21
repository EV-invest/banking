import { RevenueView } from "@/views/admin/revenue/ui/revenue-view";

// Admin console — the fund's earned revenue and its on-chain payouts. Authorized
// server-side; the BFF re-checks `RevenuePayout` (Admin/Owner) at the money plane.
export default function AdminRevenuePage() {
  return <RevenueView />;
}

import { RevenueView } from "@/views/admin/revenue/ui/revenue-view";

// Admin console — the platform's earned revenue, the reserved `fee` allocation.
// Authorized server-side; the BFF re-checks the admin role at the money plane.
export default function AdminRevenuePage() {
  return <RevenueView />;
}

import { FeesView } from "@/views/admin/fees/ui/fees-view";

// Admin console — fee terms per fund, and settling what they have earned. Authorized
// server-side: the BFF admits only the `admin` and `owner` roles to `/api/admin/fees/*`
// (an operator gets 403, which the view renders as its own state — banking#269), and the
// money plane re-checks `AllocationManage` behind it.
export default function AdminFeesPage() {
  return <FeesView />;
}

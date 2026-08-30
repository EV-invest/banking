import { FeesView } from "@/views/admin/fees/ui/fees-view";

// Admin console — fee terms per fund, and settling what they have earned. Authorized
// server-side; the BFF re-checks `AllocationManage` at the money plane.
export default function AdminFeesPage() {
  return <FeesView />;
}

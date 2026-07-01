import { OverviewView } from "@/views/admin/overview/ui/overview-view";

// Admin console — fleet health & throughput. Access is authorized server-side by the
// BFF admin routes (role-gated); the nav is hidden for non-operators.
export default function AdminOverviewPage() {
  return <OverviewView />;
}

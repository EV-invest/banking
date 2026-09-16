import { OutboxView } from "@/views/admin/outbox/ui/outbox-view";

// Admin console — parked outbox rows and the unpark action. Fleet health and the relay
// KPIs live in Grafana; this stays because unparking is an action, not a reading.
// Authorized server-side by the BFF admin routes (role-gated); the nav is hidden for
// non-operators.
export default function AdminOutboxPage() {
  return <OutboxView />;
}

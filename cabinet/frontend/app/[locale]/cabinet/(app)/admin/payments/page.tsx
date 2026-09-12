import { PaymentsView } from "@/views/admin/payments/ui/payments-view";

// Admin console — payment orders between the platform's claims and out to a chain.
// Authorized server-side; the BFF re-checks `PaymentOpen` (Admin/Owner) at the money plane.
export default function AdminPaymentsPage() {
  return <PaymentsView />;
}

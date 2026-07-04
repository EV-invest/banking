import { WithdrawalsView } from "@/views/admin/withdrawals/ui/withdrawals-view";

// Admin console — the withdrawal operator queue (dispatch / settle / fail). Authorized server-side.
export default function AdminWithdrawalsPage() {
  return <WithdrawalsView />;
}

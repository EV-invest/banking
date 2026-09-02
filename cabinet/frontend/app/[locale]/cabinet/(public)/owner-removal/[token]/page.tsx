import { RemovalApprovalView } from "@/views/approval/ui/removal-approval-view";

// The "your ownership is being ended" notice, opened by the owner it is about.
// Same contract as the payout approval next door: the token is handed to the client and
// read from the browser, and the page is never prerendered or cached.
export const dynamic = "force-dynamic";

export default async function OwnerRemovalApprovalPage({ params }: { params: Promise<{ token: string }> }) {
  const { token } = await params;
  return <RemovalApprovalView token={token} />;
}

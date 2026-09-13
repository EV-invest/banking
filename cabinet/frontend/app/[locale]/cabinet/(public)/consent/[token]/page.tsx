import { ConsentApprovalView } from "@/views/approval/ui/consent-approval-view";

// The payment consent an investor opens from their email — their own money, their answer.
// Same contract as the payout approval next door: the token is handed to the client and
// read from the browser, and the page is never prerendered or cached — every token is a
// different page, each is single-use, and a cached one would be served to the next reader.
export const dynamic = "force-dynamic";

export default async function ConsentApprovalPage({ params }: { params: Promise<{ token: string }> }) {
  const { token } = await params;
  return <ConsentApprovalView token={token} />;
}

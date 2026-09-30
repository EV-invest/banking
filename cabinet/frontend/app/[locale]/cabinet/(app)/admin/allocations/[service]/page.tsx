import { notFound } from "next/navigation";

import { isServiceId } from "@/entities/scope/lib/access";
import { PanelAccessView } from "@/views/admin/panel-access/ui/panel-access-view";

// One allocation's panel access, on a page of its own so a scope's admin — who holds no
// console role and cannot read the registry — has somewhere to manage it. Authorized by
// the identity plane on every call.
export default async function AllocationPanelAccessPage({ params }: { params: Promise<{ service: string }> }) {
  // Not decoded: a valid id has nothing to decode, and one that needs it is not valid.
  const { service } = await params;
  if (!isServiceId(service)) notFound();
  return <PanelAccessView service={service} />;
}

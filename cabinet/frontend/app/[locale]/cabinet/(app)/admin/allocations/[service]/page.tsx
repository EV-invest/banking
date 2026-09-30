import { PanelAccessView } from "@/views/admin/panel-access/ui/panel-access-view";

// One allocation's panel access, on a page of its own so a scope's admin — who holds no
// console role and cannot read the registry — has somewhere to manage it. Authorized by
// the identity plane on every call.
export default async function AllocationPanelAccessPage({ params }: { params: Promise<{ service: string }> }) {
  const { service } = await params;
  return <PanelAccessView service={decodeURIComponent(service)} />;
}

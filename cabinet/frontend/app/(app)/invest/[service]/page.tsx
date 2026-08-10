import { ProductView } from "@/views/invest/ui/product-view";

// One product per page. The slug is the allocation's `service` id — the same key every
// fund RPC takes — so the URL is the product's identity rather than an index into a list
// that reorders. An unregistered slug renders the not-found state; the hub refuses it
// too, so a wrong link can never become a surface that deals.
export default async function ProductPage({ params }: { params: Promise<{ service: string }> }) {
  const { service } = await params;
  return <ProductView service={decodeURIComponent(service)} />;
}

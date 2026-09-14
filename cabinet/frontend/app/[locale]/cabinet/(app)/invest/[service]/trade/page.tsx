import { TradeView } from "@/views/trade/ui/trade-view";

// The terminal for one product's book, keyed by the same `service` slug as the product
// page above it. Thin on purpose: the page is the route, the view is the screen.
export default async function TradePage({ params }: { params: Promise<{ service: string }> }) {
  const { service } = await params;
  return <TradeView service={decodeURIComponent(service)} />;
}

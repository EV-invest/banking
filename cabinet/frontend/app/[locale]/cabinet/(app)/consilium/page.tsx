import { ConsiliumView } from "@/views/consilium/ui/consilium-view";

// The owners' room. Gated by the session like every other `(app)` page; the BFF gates it
// again on the caller actually being an owner, and answers 403 to anyone who is not.
export default function ConsiliumPage() {
  return <ConsiliumView />;
}

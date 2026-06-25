import type { ReactNode } from "react";

// Full-bleed wrapper for the unauthenticated surfaces. Each view owns its own framing —
// login is an edge-to-edge two-panel, logged-out centers a card. No app shell here; the
// sidebar belongs to the signed-in `(app)` route group.
export default function AuthLayout({ children }: { children: ReactNode }) {
  return <div className="min-h-screen bg-background">{children}</div>;
}

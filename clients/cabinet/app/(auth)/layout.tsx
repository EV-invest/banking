import type { ReactNode } from "react";

// Chrome-light wrapper for the auth screens: centers the card in the viewport
// (below the shared header from the root layout).
export default function AuthLayout({ children }: { children: ReactNode }) {
  return <div className="flex min-h-[calc(100vh-4rem)] items-center justify-center px-6 py-16">{children}</div>;
}

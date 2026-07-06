"use client";

import { ServerError } from "@evinvest/uikit";

// Route-segment error boundary (500). The shared surface from @evinvest/uikit;
// "Try again" runs Next's `reset`, and "back to home" returns to the dashboard.
export default function Error({ reset }: { error: Error & { digest?: string }; reset: () => void }) {
  return <ServerError reset={reset} homeHref="/cabinet" />;
}

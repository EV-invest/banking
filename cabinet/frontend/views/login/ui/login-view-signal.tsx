"use client";

// The one client island on the sign-in page: records `login_view`. Renders nothing.
//
// The view itself is a server component, so the capture has to live in a leaf of its own
// rather than turn the whole page client-side. Props are flags, never the values: a
// `returnTo` is a URL, and the error code is the shell's business — neither belongs in an
// event.

import { useAnalytics } from "@evinvest/analytics/react";
import { useEffect } from "react";

import { ACTIVATION, mark } from "@/shared/analytics";

export function LoginViewSignal({ hasReturnTo, hasError }: { hasReturnTo: boolean; hasError: boolean }) {
  const capture = useAnalytics();
  useEffect(() => {
    // `session_created` reads this back to say whether the sign-in began on this page.
    mark("login_view");
    capture(ACTIVATION.loginView, { has_return_to: hasReturnTo, has_error: hasError });
  }, [capture, hasReturnTo, hasError]);
  return null;
}

// Server/runtime error monitoring (Sentry) for the BFF. `register` runs once on
// server boot and initialises the matching runtime (Node here — Next 16 proxy is
// Node-only) via @evinvest/error-monitoring; `onRequestError` reports errors
// thrown in server components and route handlers. Both no-op without SENTRY_DSN.
import {
  register as initMonitoring,
  captureRequestError,
} from "@evinvest/error-monitoring/next";
import { assertConfig } from "@/config";

export function register() {
  // Fail the boot, not the request: a required var missing throws here,
  // before the server accepts traffic.
  assertConfig();
  return initMonitoring();
}

export const onRequestError = captureRequestError;

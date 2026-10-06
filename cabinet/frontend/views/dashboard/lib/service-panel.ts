// Whether Home offers the Service-Arb panel, and to whom its access page is offered too.
// Pure so the gate is tested apart from the card.

// Relative and with the extension: the node test runner resolves no `@/` alias.
import { TENANTS, canOpenPanel } from "../../../entities/grant/lib/access.ts";

export const SERVICE_ARB = "service_arb";
export const SERVICE_ARB_TENANT = TENANTS[SERVICE_ARB];

export interface ServicePanelLink {
  href: string;
  /** The reader also administers who may open it. */
  manage: boolean;
}

/** `null` when there is no panel to link to (unconfigured) or the reader may not open it. */
export function servicePanelLink(url: string | undefined, permissions: readonly string[], manage: boolean): ServicePanelLink | null {
  if (!url || !canOpenPanel(permissions, SERVICE_ARB_TENANT)) return null;
  return { href: url, manage };
}

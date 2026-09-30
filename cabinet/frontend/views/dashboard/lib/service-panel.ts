// Whether Home offers the Service-Arb panel, and to whom its access page is offered too.
// Pure so the gate is tested apart from the card.

// Relative and with the extension: the node test runner resolves no `@/` alias.
import { canManagePanelAccess, canOpenPanel, type GrantLike } from "../../../entities/scope/lib/access.ts";

/** The allocation the panel serves — its scope is `allocation:service_arb`. */
export const SERVICE_ARB = "service_arb";

export interface ServicePanelLink {
  href: string;
  /** The reader also administers who may open it. */
  manage: boolean;
}

/** `null` when there is no panel to link to (unconfigured) or the reader may not open it. */
export function servicePanelLink(url: string | undefined, globalRole: string | undefined, scopes: readonly GrantLike[]): ServicePanelLink | null {
  if (!url || !canOpenPanel(globalRole, scopes, SERVICE_ARB)) return null;
  return { href: url, manage: canManagePanelAccess(globalRole, scopes, SERVICE_ARB) };
}

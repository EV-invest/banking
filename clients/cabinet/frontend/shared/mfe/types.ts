// The microfrontend registry contract. Client-safe (no server imports) so both
// the BFF and client components can share these types.

export type MfeKind = "component" | "page";

export interface MfeEntry {
  /** Logical name, e.g. "risk.pnl-chart". */
  name: string;
  /** Globally-unique, versioned custom-element tag, e.g. "mfe-risk-pnl-chart". */
  tag: string;
  /** URL of the remote's self-registering ESM bundle (its own origin/CDN). */
  scriptUrl: string;
  /** Whether the remote is an inline widget or owns a whole route. */
  kind: MfeKind;
}

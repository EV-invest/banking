// Brand-header navigation — the same items the conductor's own header carries,
// as root-relative hard links: under the zone mount the cabinet shares the
// conductor's domain, so "/team" etc. resolve to the conductor's pages (a full
// document load — cross-zone nav must never go through next/link). When the
// cabinet is opened directly on its own origin in dev these 404 outside the
// basePath; through the conductor they are correct.
export const CONDUCTOR_NAV = [
  { label: "Portfolio", href: "/#portfolio" },
  { label: "Research", href: "/#research" },
  { label: "Team", href: "/team" },
  { label: "Hiring", href: "/hiring" },
  { label: "Contact", href: "/contact" },
] as const;

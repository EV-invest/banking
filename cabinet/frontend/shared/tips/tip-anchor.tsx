"use client";

import {
  InfoTip,
  InfoTipContent,
  InfoTipTrigger,
  SectionDescriptor,
} from "@evinvest/uikit";

import { useSession } from "@/shared/lib/use-session";

import { tips, type TipKey } from "./catalog";

export interface TipAnchorProps {
  /** The catalog key to render. Checked against the catalog at compile time. */
  anchor: TipKey;
  className?: string;
}

/**
 * Renders the tip registered under `anchor`: an inline ⓘ toggletip for
 * `type: "input"` entries, or a section descriptor block for `type: "section"`.
 * All copy comes from the catalog — the uikit engine stays content-agnostic.
 *
 * A catalog `roles` gate is enforced against the session role (cosmetic — server
 * authz stays authoritative), so an operator-only tip never renders for
 * investors. While the session is still loading a gated tip stays hidden.
 */
export function TipAnchor({ anchor, className }: TipAnchorProps) {
  const session = useSession();
  const entry = tips[anchor];

  if (entry.roles) {
    const role = session?.user?.role;
    if (!role || !entry.roles.includes(role)) return null;
  }

  if (entry.type === "section") {
    return (
      <SectionDescriptor title={entry.title} className={className}>
        {entry.body}
      </SectionDescriptor>
    );
  }

  return (
    <InfoTip>
      <InfoTipTrigger label={`About: ${entry.title}`} className={className} />
      <InfoTipContent>
        <p className="text-foreground font-medium">{entry.title}</p>
        <p className="text-muted-foreground mt-1">{entry.body}</p>
      </InfoTipContent>
    </InfoTip>
  );
}

"use client";

import { Users, Globe } from "lucide-react";
import { notifyPlaceholder } from "@/shared/lib/utils";
import { useExperimentEvent } from "@/features/ab-variant";
import { PlaceholderCard } from "./cards";

/**
 * Client island — the two interactive "join us" / "LP network" CTA cards.
 *
 * Isolated so the rest of the Team section (intro, image, member grid) stays a
 * Server Component. Uses {@link useExperimentEvent} to attribute clicks to the
 * active experiment variant via the {@link ExperimentTracker} wrapping the section.
 * Rendered as a fragment so the cards sit as direct children of the parent grid.
 */
export function TeamPlaceholders() {
  const track = useExperimentEvent();
  return (
    <>
      <PlaceholderCard
        icon={Users}
        iconClassName="text-main-accent-t1"
        title="Join Us"
        body="We are always looking for talented analysts and asset managers in Quy Nhon."
        cta="Careers"
        onCtaClick={() =>
          track("cta_clicked", { cta: "careers" }, (fire) => {
            fire();
            notifyPlaceholder("Careers");
          })
        }
        heading="Open Position"
        sub="Investment Analyst"
      />

      <PlaceholderCard
        icon={Globe}
        iconClassName="text-main-accent-t1"
        title="LP Partner Network"
        body="Over 40 institutional investors across 12 countries trust us with their capital."
        cta="IR Contacts"
        onCtaClick={() =>
          track("cta_clicked", { cta: "ir_contacts" }, (fire) => {
            fire();
            notifyPlaceholder("IR Contacts");
          })
        }
        heading="Investor Relations"
        sub="Investor Relations (IR)"
      />
    </>
  );
}

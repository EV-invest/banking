"use client";

import { ArrowRight } from "lucide-react";
import { Button } from "@evinvest/uikit";
import { useExperimentEvent } from "@/features/ab-variant";

/**
 * Client island — the "Explore Assets" button for Hero B.
 * Extracted so HeroB stays a Server Component.
 */
export function HeroBCta({ className }: { className?: string }) {
  const track = useExperimentEvent();
  return (
    <Button
      className={className}
      onClick={() =>
        track("cta_clicked", { cta: "explore_assets" }, (fire) => {
          fire();
          document.getElementById("portfolio")?.scrollIntoView({ behavior: "smooth" });
        })
      }
    >
      Explore Assets <ArrowRight className="w-4 h-4 ml-2" />
    </Button>
  );
}

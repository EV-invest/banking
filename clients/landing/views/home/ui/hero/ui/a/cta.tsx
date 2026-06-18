"use client";

import { ArrowRight } from "lucide-react";
import { Button } from "@evinvest/uikit";
import { Text } from "@/shared/ui/text";
import { useExperimentEvent } from "@/features/ab-variant";

/**
 * Client island — the "Explore Assets" button and scroll hint.
 * Extracted from HeroA so the rest of the section stays a Server Component.
 */
export function HeroACta() {
  const track = useExperimentEvent();
  return (
    <>
      <Button
        className="bg-main-mist text-main-brand hover:bg-main-accent-t1 hover:text-main-black hover:scale-105 active:scale-95 transition-all duration-300 font-mono-tech text-xs tracking-widest uppercase px-8 py-6 rounded-none"
        onClick={() =>
          track("cta_clicked", { cta: "explore_assets" }, (fire) => {
            fire();
            document.getElementById("portfolio")?.scrollIntoView({ behavior: "smooth" });
          })
        }
      >
        Explore Assets <ArrowRight className="w-4 h-4 ml-2" />
      </Button>

      <Text asChild variant="secondary" className="mt-8">
        <span className="text-[9px] font-mono-tech tracking-[0.3em] uppercase">
          Scroll to zoom in &amp; discover
        </span>
      </Text>
    </>
  );
}

"use client";

import { cn } from "@/shared/lib/cn";
import { networkLabel } from "@/views/wallet/lib/format";

// The full-width segmented rail picker (Figma `seg-*`): equal-width segments on a sunken
// surface track, the selection carrying the teal fill. Rails come from the wallet response,
// so the segment count varies — hence flex-1 segments rather than a fixed grid.
export function NetworkSegments({ networks, value, onChange, label }: { networks: string[]; value: string; onChange: (network: string) => void; label: string }) {
  return (
    <div role="radiogroup" aria-label={label} className="flex w-full rounded-[10px] bg-main-surface p-1">
      {networks.map((network) => {
        const selected = network === value;
        return (
          <button
            key={network}
            type="button"
            role="radio"
            aria-checked={selected}
            onClick={() => onChange(network)}
            className={cn(
              "min-w-0 flex-1 rounded-lg py-2 text-xs font-medium transition-colors lg:py-[9px] lg:text-[13px]",
              selected ? "bg-main-accent-t1 text-main-black" : "text-main-mist hover:text-white",
            )}
          >
            {networkLabel(network)}
          </button>
        );
      })}
    </div>
  );
}

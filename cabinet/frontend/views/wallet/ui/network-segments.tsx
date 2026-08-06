"use client";

import { cn } from "@/shared/lib/cn";
import { networkLabel } from "@/views/wallet/lib/format";

// The full-width segmented rail picker (Figma `seg-*`): equal-width segments on a sunken
// surface track, the selection carrying the teal fill. Rails come from the wallet response,
// so the segment count varies — hence flex-1 segments rather than a fixed grid.
// Hand-written — uikit has no equivalent, so the segments carry their own focus ring.
export function NetworkSegments({ networks, value, onChange, label }: { networks: string[]; value: string; onChange: (network: string) => void; label: string }) {
  return (
    <div role="radiogroup" aria-label={label} className="flex w-full rounded-lg bg-main-surface p-1">
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
              "min-w-0 flex-1 rounded-md py-2 text-xs font-medium outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring/50 lg:py-2.5 lg:text-sm",
              selected ? "bg-main-accent-t1 text-main-black" : "text-muted-foreground hover:text-foreground",
            )}
          >
            {networkLabel(network)}
          </button>
        );
      })}
    </div>
  );
}

"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import {
  experiments,
  cookieName,
  type ExperimentKey,
} from "@/shared/config/experiments";
import { readCookie, writeVariant } from "./cycle-variant";

/**
 * Development-only overlay for overriding experiment variants.
 *
 * Renders a floating panel with buttons for each experiment × variant pair.
 * Clicking a button writes the chosen value to the `ab_<key>` cookie and calls
 * `router.refresh()`, so the server re-renders the new variant without a full
 * page reload. Returns `null` in production — zero cost at runtime.
 *
 * Cookies are read only after mount (never during render) to avoid a hydration
 * mismatch: the server has no `document.cookie`, so reading it synchronously
 * would produce different markup on client vs. server.
 *
 * New experiments added to `shared/config/experiments.ts` appear automatically.
 */
export function DevAbPanel() {
  const router = useRouter();
  const [active, setActive] = useState<Partial<Record<ExperimentKey, string>>>(
    {},
  );

  useEffect(() => {
    const next: Partial<Record<ExperimentKey, string>> = {};
    for (const key of Object.keys(experiments) as ExperimentKey[]) {
      next[key] = readCookie(cookieName(key));
    }
    setActive(next);
  }, []);

  if (process.env.NODE_ENV !== "development") return null;

  const setVariant = (key: ExperimentKey, value: string) => {
    writeVariant(key, value);
    setActive(prev => ({ ...prev, [key]: value }));
    router.refresh();
  };

  return (
    <div className="fixed bottom-3 right-3 z-50 flex flex-col gap-2 rounded-lg border border-main-mist/15 bg-main-black/90 p-3 font-mono-tech text-[10px] text-main-mist/80 shadow-2xl backdrop-blur">
      <span className="uppercase tracking-[0.2em] text-main-accent-t1">
        A/B
      </span>
      {(Object.keys(experiments) as ExperimentKey[]).map(key => (
        <div key={key} className="flex items-center gap-2">
          <span className="w-16 truncate text-main-mist/50">{key}</span>
          {experiments[key].variants.map(v => (
            <button
              key={v}
              type="button"
              onClick={() => setVariant(key, v)}
              className={`rounded px-2 py-0.5 uppercase transition-colors ${
                active[key] === v
                  ? "bg-main-accent-t1 text-main-black"
                  : "border border-main-mist/20 hover:border-main-accent-t1"
              }`}
            >
              {v}
            </button>
          ))}
        </div>
      ))}
    </div>
  );
}

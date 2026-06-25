import type { ReactNode } from "react";

// A styled, navigable "not built yet" surface for routes whose Figma screens are designed
// but not yet implemented (Operations · Settings). Keeps the shell coherent — these are
// real nav destinations, not dead links or 404s — until their views land.
export function PagePlaceholder({ eyebrow, title, blurb, icon }: { eyebrow: string; title: string; blurb: string; icon: ReactNode }) {
  return (
    <div className="px-8 pb-7 pt-6">
      <header className="mb-6 space-y-1">
        <p className="font-mono-tech text-xs uppercase tracking-widest text-main-accent-t1">{eyebrow}</p>
        <h1 className="font-sans text-2xl font-semibold text-foreground">{title}</h1>
      </header>
      <div className="flex flex-col items-center gap-3 rounded-[14px] border border-border bg-main-card px-8 py-20 text-center">
        <span className="flex size-12 items-center justify-center rounded-full bg-main-accent-t1/10 text-main-accent-t1">{icon}</span>
        <p className="max-w-md text-sm text-muted-foreground">{blurb}</p>
      </div>
    </div>
  );
}

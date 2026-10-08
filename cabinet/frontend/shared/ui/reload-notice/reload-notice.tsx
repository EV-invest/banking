"use client";

// What a surface shows when a piece of its own code never downloaded (a ChunkLoadError,
// `shared/lib/chunk-error`). Not `ResourceError`: its "Try again" re-runs a read, and here
// there is nothing to re-run — the bundler keeps the failed chunk for the life of the page,
// so the one action that can work is a reload.

import { RefreshCw } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Button } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";

export function ReloadNotice({ className }: { className?: string }) {
  const t = useT();
  return (
    <div className={cn("flex flex-col items-center justify-center gap-3 px-6 text-center", className)}>
      <p className="text-xs text-ink-soft">{t("err.chunkLoad", "This part of the page didn't load.")}</p>
      <Button type="button" variant="outline" size="sm" onClick={() => window.location.reload()}>
        <RefreshCw className="size-4" /> {t("ui.reloadPage", "Reload page")}
      </Button>
    </div>
  );
}

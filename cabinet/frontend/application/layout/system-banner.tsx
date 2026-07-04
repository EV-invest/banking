"use client";

import { TriangleAlert, X } from "lucide-react";
import { useState } from "react";

import { usePlatform } from "@/shared/lib/use-platform";

const DISMISS_KEY = "ev-announcement-dismissed";

function storedDismissal(): string | null {
  try {
    return typeof window === "undefined" ? null : window.sessionStorage.getItem(DISMISS_KEY);
  } catch {
    return null;
  }
}

// Slim full-width system strips above the signed-in page content: maintenance and
// read-only states (amber, not dismissible) plus the operator announcement (neutral,
// dismissible per tab-session — keyed on title+body so an EDITED announcement
// re-appears). Best-effort by design: no platform data renders nothing.
export function SystemBanner() {
  const platform = usePlatform();
  const [dismissedKey, setDismissedKey] = useState<string | null>(storedDismissal);

  if (!platform) return null;

  const announcementKey = `${platform.announcement_title}\u0000${platform.announcement_body}`;
  const showAnnouncement = platform.announcement_active && dismissedKey !== announcementKey;
  if (!platform.maintenance_mode && !platform.read_only && !showAnnouncement) return null;

  const dismiss = () => {
    try {
      window.sessionStorage.setItem(DISMISS_KEY, announcementKey);
    } catch {
      // Storage unavailable (private mode) — dismissal still holds for this render tree.
    }
    setDismissedKey(announcementKey);
  };

  return (
    <div className="space-y-2 px-8 pt-4">
      {platform.maintenance_mode && <AmberStrip>Scheduled maintenance — some actions may be briefly unavailable.</AmberStrip>}
      {platform.read_only && <AmberStrip>Withdrawals and investments are temporarily paused (read-only mode). Balances are unaffected.</AmberStrip>}
      {showAnnouncement && (
        <div className="flex items-start gap-3 rounded-lg border border-border bg-main-card px-4 py-2.5 text-sm">
          <div className="min-w-0 flex-1">
            <span className="font-semibold">{platform.announcement_title}</span>
            {platform.announcement_body && <span className="text-muted-foreground"> — {platform.announcement_body}</span>}
          </div>
          <button type="button" aria-label="Dismiss announcement" onClick={dismiss} className="shrink-0 text-muted-foreground transition-colors hover:text-foreground">
            <X className="size-4" />
          </button>
        </div>
      )}
    </div>
  );
}

function AmberStrip({ children }: { children: string }) {
  return (
    <div className="flex items-center gap-2.5 rounded-lg border border-main-accent-t3/40 bg-main-accent-t3/10 px-4 py-2.5 text-sm text-main-accent-t3">
      <TriangleAlert className="size-4 shrink-0" />
      {children}
    </div>
  );
}

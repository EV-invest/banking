"use client";

// Sessions & devices: the live refresh-token families at the hub, listed and revocable.
// One list serves both breakpoints — the desktop rail renders it titled, the mobile stack
// pushes it as its own screen under the app bar.

import { Loader2 } from "lucide-react";

import { Button, Skeleton } from "@evinvest/uikit";

import type { Session } from "@/shared/contracts";
import { TipAnchor } from "@/shared/tips";
import { Hairline, ListCard, ListCardTitle, Pill } from "@/shared/ui/list-card";
import { deviceOf, metaOf } from "@/views/settings/lib/sessions";

export function SessionsSection({
  titled,
  sessions,
  error,
  busy,
  name,
  onRevoke,
  onRevokeOthers,
}: {
  titled: boolean;
  sessions: Session[] | undefined;
  error: string | null;
  busy: boolean;
  name: string;
  onRevoke: (id: string) => void;
  onRevokeOthers: () => void;
}) {
  const loading = sessions === undefined;
  const list = sessions ?? [];
  const hasOthers = list.some((s) => !s.current);
  return (
    <ListCard className="lg:px-6 lg:pb-5.5 lg:pt-2">
      {titled ? (
        <ListCardTitle sub="Where you're signed in — revoke anything you don't recognise">Sessions &amp; devices</ListCardTitle>
      ) : (
        <p className="pb-2 pt-3 text-xs font-medium text-muted-foreground">Where you&apos;re signed in — revoke anything you don&apos;t recognise</p>
      )}

      {error && <p className="mb-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</p>}

      {loading ? (
        [0, 1].map((i) => (
          <div key={i}>
            {i > 0 && <Hairline />}
            <div className="flex items-center gap-3 py-3.5">
              <Skeleton className="size-9 shrink-0 rounded-lg" />
              <Skeleton className="h-4 flex-1" />
              <Skeleton className="h-8 w-20 shrink-0 rounded-md" />
            </div>
          </div>
        ))
      ) : list.length === 0 ? (
        <p className="py-6 text-center text-sm text-muted-foreground">No active sessions.</p>
      ) : (
        list.map((s, i) => {
          const { label, icon: Icon } = deviceOf(s.user_agent);
          return (
            <div key={s.id ?? i}>
              {i > 0 && <Hairline />}
              {/* Device and action stack below `sm` — side by side, both the device label and
                  the ip/last-seen meta were being clipped to an ellipsis on a phone. */}
              <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2.5 py-3.5">
                <div className="flex min-w-0 items-start gap-3">
                  <span className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-border bg-main-surface text-foreground">
                    <Icon className="size-4.5" />
                  </span>
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-medium text-foreground">{label}</p>
                    <p className="break-words text-xs leading-snug text-muted-foreground">{metaOf(s, name)}</p>
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1.5 pl-12 sm:pl-0">
                  {s.current ? (
                    <>
                      <Pill>This device</Pill>
                      <TipAnchor anchor="settings.sessions.this-device" />
                    </>
                  ) : (
                    <>
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={busy}
                        onClick={() => s.id && onRevoke(s.id)}
                        className="border-main-accent-t4/40 text-main-accent-t4 hover:text-main-accent-t4"
                      >
                        Revoke
                      </Button>
                      <TipAnchor anchor="settings.sessions.revoke" />
                    </>
                  )}
                </div>
              </div>
            </div>
          );
        })
      )}

      {!loading && hasOthers && (
        <div className="mb-2 mt-3 flex items-center gap-1.5">
          <Button
            variant="outline"
            disabled={busy}
            onClick={onRevokeOthers}
            className="w-full border-main-accent-t4/40 text-main-accent-t4 hover:text-main-accent-t4"
          >
            {busy && <Loader2 className="mr-1.5 size-4 animate-spin" />} Sign out all other devices
          </Button>
          <TipAnchor anchor="settings.sessions.revoke-others" />
        </div>
      )}
    </ListCard>
  );
}

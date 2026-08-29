"use client";

// What a screen shows when a read it needed never arrived.
//
// Ten views each derived a read's failure — most as `data ? null : (read.error?.message ??
// null)`, some with an action error folded in first — and then hand-rolled the banner for
// it. Nothing kept those banners in step, so the same event was reported in three different
// looks depending on which screen you were on, and a fix to one never reached the others.
// Deriving the message stays with the caller; only the looks are stated once, here.
//
// Two forms, because the console and the investor screens report a failure at different
// weights and both are deliberate:
//
//   `inline` — one destructive line under the page header. The admin console's screens are
//              dense grids of figures; a bordered block above them pushes the whole page
//              down for a message that is usually one clause long.
//   `alert`  — uikit's `Alert`, with a heading. The investor screens address the reader
//              ("Couldn't load your positions"), so the message needs a subject and the
//              block gives it one.
//
// `onRetry` boxes the inline form. A button cannot sit in a bare line of text without the
// two reading as unrelated things that happen to be adjacent, so the row and the button
// get a border around them and become one block.
//
// It renders its own `StaggerItem` for the same reason `AdminHeader` does: this is a
// section of the page, it arrives with the rest of them, and a caller that had to remember
// to wrap it would be a caller that could forget. Nothing is wrapped in a new element —
// the inline form animates the `<p>` itself (see `shared/ui/motion`, where the rule and
// the layout it protects are spelled out).
//
// One page-level banner is deliberately not on this: wallet Activity reports its failure
// as bare text with no icon. That is a difference in what is drawn, not in how it is
// built, so aligning it is a UI decision rather than something to smuggle in here.

import { RefreshCw, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";

import { Alert, AlertDescription, AlertTitle, Button } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";
import { StaggerItem } from "@/shared/ui/motion";

interface ResourceErrorBase {
  /** The failure, already derived — the views own how they read it off a resource. */
  message: ReactNode;
  /** Placement, on whichever element this form's root is. */
  className?: string;
}

// The two forms take different props, and the union says so rather than documenting it: an
// `alert` has no retry button and an `inline` line has no heading, so asking for either is a
// type error at the call site instead of a prop that silently does nothing.
export type ResourceErrorProps = ResourceErrorBase &
  (
    | {
        variant?: "inline";
        title?: never;
        /** Re-run the read. Present only where the screen has a read worth retrying by itself. */
        onRetry?: () => void;
        /** The retry is in flight: the button is disabled and its icon spins. */
        retrying?: boolean;
      }
    | {
        variant: "alert";
        /** Heading for the `alert` form. */
        title?: ReactNode;
        onRetry?: never;
        retrying?: never;
      }
  );

/**
 * The banner for a read that failed with nothing to show in its place.
 *
 * Whether there is anything to report stays with the caller — only it knows whether stale
 * data is still on screen, and a failed *refresh* over figures the reader can already see
 * belongs beside them rather than in place of them.
 */
export function ResourceError({ message, variant = "inline", title, onRetry, retrying = false, className }: ResourceErrorProps) {
  if (variant === "alert") {
    return (
      <StaggerItem className={className}>
        <Alert variant="destructive">
          <TriangleAlert className="size-4" />
          {title && <AlertTitle>{title}</AlertTitle>}
          <AlertDescription>{message}</AlertDescription>
        </Alert>
      </StaggerItem>
    );
  }

  if (onRetry) {
    return (
      <StaggerItem className={cn("flex flex-wrap items-center gap-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2.5", className)}>
        {/* The icon holds its size here: beside the button it is the first thing squeezed
            when the message is long, and a half-width triangle reads as a rendering fault. */}
        <p className="flex items-center gap-2 text-sm text-destructive">
          <TriangleAlert className="size-4 shrink-0" /> {message}
        </p>
        <Button type="button" variant="outline" size="sm" disabled={retrying} onClick={onRetry}>
          <RefreshCw className={retrying ? "size-4 animate-spin" : "size-4"} /> Try again
        </Button>
      </StaggerItem>
    );
  }

  return (
    <StaggerItem as="p" className={cn("flex items-center gap-2 text-sm text-destructive", className)}>
      <TriangleAlert className="size-4" /> {message}
    </StaggerItem>
  );
}

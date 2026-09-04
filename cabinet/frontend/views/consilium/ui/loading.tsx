"use client";

// What each card stands in for while its read is in flight.
//
// All four used to be the same `<Skeleton className="h-40 w-full" />`: one flat slab under a
// title and a description. On a dark card a featureless rectangle does not read as "this is
// coming" — it reads as "there is nothing here", which is how a page that was merely slow
// got reported as a page of empty boxes. A skeleton earns its place by having the SHAPE of
// what it stands in for, so the card is recognisable before its content lands and nothing
// jumps when it does.
//
// Sizes are the scale's, not measurements of the real content: this is a placeholder, and a
// placeholder that tracks its subject to the pixel is a second copy of the layout to keep in
// step. `Skeleton` brings its own fade (`[data-slot="skeleton"]` in globals.css).

import { Skeleton } from "@evinvest/uikit";

/** Three owners: avatar, name, "owner since". */
export function RosterSkeleton() {
  return (
    <div className="flex flex-col gap-4">
      {[0, 1, 2].map((row) => (
        <div key={row} className="flex items-center gap-3">
          <Skeleton className="size-8 shrink-0 rounded-full" />
          <div className="flex min-w-0 flex-1 flex-col gap-1.5">
            <Skeleton className="h-3.5 w-40 max-w-full" />
            <Skeleton className="h-3 w-24 max-w-full" />
          </div>
        </div>
      ))}
    </div>
  );
}

/** An open payout: the amount, the address block, its meta lines, the tally bar. */
export function PayoutSkeleton() {
  return (
    <div className="flex flex-col gap-4">
      <Skeleton className="h-8 w-48 max-w-full" />
      <Skeleton className="h-10 w-full rounded-lg" />
      <div className="flex flex-col gap-2">
        <Skeleton className="h-3 w-56 max-w-full" />
        <Skeleton className="h-3 w-40 max-w-full" />
      </div>
      <Skeleton className="h-1.5 w-full rounded-full" />
    </div>
  );
}

/** One removal: who it is about, the reason block, where the votes stand. */
export function RemovalsSkeleton() {
  return (
    <div className="flex flex-col gap-4">
      <Skeleton className="h-4 w-56 max-w-full" />
      <Skeleton className="h-16 w-full rounded-lg" />
      <div className="flex flex-col gap-2">
        <Skeleton className="h-3 w-44 max-w-full" />
        <Skeleton className="h-3 w-32 max-w-full" />
      </div>
    </div>
  );
}

/** One admission: who it is about, the reason block, where the peers stand. */
export function AdmissionsSkeleton() {
  return (
    <div className="flex flex-col gap-4">
      <Skeleton className="h-4 w-64 max-w-full" />
      <Skeleton className="h-16 w-full rounded-lg" />
      <div className="flex flex-col gap-2">
        <Skeleton className="h-3 w-48 max-w-full" />
        <Skeleton className="h-3 w-36 max-w-full" />
      </div>
    </div>
  );
}

/** The form: owner picker, reason, submit. Also stands in for the admission form, whose
 *  shape is the same three rows — a single-line field, a reason box and a button. */
export function ProposeSkeleton() {
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-2">
        <Skeleton className="h-3 w-16" />
        <Skeleton className="h-9 w-full rounded-md" />
      </div>
      <div className="flex flex-col gap-2">
        <Skeleton className="h-3 w-16" />
        <Skeleton className="h-20 w-full rounded-md" />
      </div>
      <Skeleton className="h-9 w-40 max-w-full rounded-md" />
    </div>
  );
}

"use client";

// The card shell every governance proposal list wears: a heading, and beneath it exactly
// one of a skeleton, a read failure, a designed zero state, or the proposals themselves.
//
// Extracted because admissions and removals had it character for character — 56 lines the
// duplication gate was right to flag. What was duplicated is worth being precise about,
// because the two halves of this feature are NOT interchangeable and most of them must
// stay apart:
//
//   · **Shared, and safe to share: the shell.** "Which of the four states is this read in,
//     and what goes in the box" is a question about a `Read<T>`, not about ownership. It
//     has the same answer for both, and having it in one place is what stops one list
//     growing a fifth state the other never gets.
//   · **Not shared, deliberately: the ballot.** `admit`/`reject` and `remove`/`keep` are
//     separate enums whose one common word inverts — `reject` KEEPS an owner on a removal
//     and REFUSES a candidate on an admission. This shell never sees a vote, a peer or a
//     verb: it takes a render function and the caller supplies its own item component,
//     with its own vote type and its own reader. There is deliberately no generic "vote"
//     parameter here for anyone to pass the wrong plane's word to.
//   · **Not shared, deliberately: the rules.** Removal has two paths (the target accepts,
//     or the eligible peers are unanimous); admission has one, unanimity of every owner
//     except the initiator with at least one such peer. Those explanations live in the
//     item bodies, where each states its own rule, rather than being flattened into one
//     generic sentence about "enough owners agreeing" — which would be true of neither.
//
// So the seam is an edge, not a rule. Sharing the frame is what keeps the two surfaces
// recognisable as one screen; sharing the ballot would be how a vote gets cast by mistake.

import type { ReactNode } from "react";
import { Fragment } from "react";

import { Card, CardContent, CardDescription, CardHeader, CardTitle, Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Separator } from "@evinvest/uikit";

import { Settled } from "@/shared/ui/motion";
import { EMPTY_BOX } from "@/views/consilium/lib/format";
import { knownValue, type Read } from "@/views/consilium/lib/reads";
import { ReadFailure } from "@/views/consilium/ui/read-failure";

export interface ProposalListProps<T> {
  /** The proposals to show, or the reason there are none to show yet. */
  read: Read<readonly T[]>;
  title: ReactNode;
  description: ReactNode;
  /** Stands in while the read is in flight — shaped like the content, not a flat slab. */
  skeleton: ReactNode;
  /** What did not load, in the reader's terms. */
  failedTitle: ReactNode;
  /** What it means for them: what this section cannot say until it loads. */
  failedBody: ReactNode;
  emptyIcon: ReactNode;
  emptyTitle: ReactNode;
  emptyBody: ReactNode;
  onRetry: () => void;
  retrying: boolean;
  /** Stable identity for one proposal. Both planes spell it `id`. */
  itemKey: (item: T) => string;
  /** One proposal, rendered by the caller. The separator between them is the shell's. */
  children: (item: T) => ReactNode;
}

export function ProposalList<T>({
  read,
  title,
  description,
  skeleton,
  failedTitle,
  failedBody,
  emptyIcon,
  emptyTitle,
  emptyBody,
  onRetry,
  retrying,
  itemKey,
  children,
}: ProposalListProps<T>) {
  // The only door out of a `Read`, and it is taken once here rather than in each caller:
  // `[]` on a read that has not landed is exactly the mistake this slice exists to
  // prevent, so the branches below never reach this value except in the `ready` case.
  const items = knownValue(read) ?? [];

  return (
    <Card>
      {/* The heading sits outside the branch on purpose. It used to render only in the
          empty case, so the moment a proposal existed the cards floated between their
          neighbours with nothing saying what they were. */}
      <CardHeader>
        <CardTitle className="text-base">{title}</CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent>
        <Settled loading={read.status === "loading"} skeleton={skeleton}>
          {read.status === "loading" ? null : read.status === "failed" ? (
            // In place of the zero state, never beside it. "Nothing is being decided" is a
            // claim about who controls the fund, and a read that failed has not earned the
            // right to make it.
            <ReadFailure title={failedTitle} body={failedBody} onRetry={onRetry} retrying={retrying} />
          ) : items.length === 0 ? (
            <Empty className={EMPTY_BOX}>
              <EmptyHeader>
                <EmptyMedia variant="icon">{emptyIcon}</EmptyMedia>
                <EmptyTitle>{emptyTitle}</EmptyTitle>
                <EmptyDescription>{emptyBody}</EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <div className="flex flex-col gap-6">
              {items.map((item, i) => (
                <Fragment key={itemKey(item)}>
                  {i > 0 && <Separator />}
                  {children(item)}
                </Fragment>
              ))}
            </div>
          )}
        </Settled>
      </CardContent>
    </Card>
  );
}

"use client";

// The people section: what the owners are deciding about one person's standing.
//
// Three kinds share one card because they share one message and one rule — a simple
// MAJORITY of the owners seated when the proposal opened. That majority is the difference
// from both consilia above it, and the copy says so rather than leaving a reader to assume
// the unanimity they have just read about twice. It is not a softer rule by accident: a
// hold lapses in 24 hours, so ratifying one races a clock, and under unanimity a single
// unreachable owner would not delay the verdict — they would decide it by releasing a
// compromised account at the deadline.
//
// What the three kinds do NOT share is the verb. The wire vote is neutral (`for`/`against`)
// precisely because one word has to serve "block this person", "let them back in" and "make
// them an admin", so the button text is resolved per kind through `proposalVoteLabel`. A
// ballot that read "For" over an unnamed act is how a vote gets cast by mistake.
//
// As everywhere in this room: nothing is optimistic. A vote posts, the tags are named, and
// the tally that comes back is the server's, counted under a row lock.

import { Loader2, UserCog } from "lucide-react";
import { Fragment, useState } from "react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Badge, Button, Item, ItemContent, ItemGroup, ItemSeparator, ItemTitle, Progress, Separator } from "@evinvest/uikit";

import { cancelUserProposal, voteOnUserProposal } from "@/entities/governance/model/governance-resource";
import type { ProposalVote, UserProposal } from "@/shared/contracts/governance";
import { errorMessage } from "@/shared/lib/api-client";
import { cn } from "@/shared/lib/cn";
import { expiresIn, formatMoment } from "@/shared/lib/datetime";
import { ResourceError } from "@/shared/ui/resource-error";
import {
  isSettled,
  proposalKindLabel,
  proposalPeerLabel,
  proposalTally,
  proposalVoteLabel,
  proposalVoteTone,
  standingInProposal,
  stateLabel,
  stateTone,
  type ProposalStanding,
} from "@/views/consilium/lib/format";
import type { Read } from "@/views/consilium/lib/reads";
import { RemovalsSkeleton } from "@/views/consilium/ui/loading";
import { ProposalList } from "@/views/consilium/ui/proposal-list";

export function UserProposalList({
  read,
  userId,
  onRetry,
  retrying,
}: {
  read: Read<UserProposal[]>;
  userId: string | null;
  onRetry: () => void;
  retrying: boolean;
}) {
  const t = useT();
  return (
    <ProposalList
      read={read}
      title={t("consilium.proposals.title")}
      description={t("consilium.proposals.sub")}
      skeleton={<RemovalsSkeleton />}
      failedTitle={t("consilium.proposals.failedTitle")}
      failedBody={t("consilium.proposals.failedBody")}
      emptyIcon={<UserCog />}
      emptyTitle={t("consilium.proposals.emptyTitle")}
      emptyBody={t("consilium.proposals.emptyBody")}
      onRetry={onRetry}
      retrying={retrying}
      itemKey={(proposal) => proposal.id}
    >
      {(proposal) => <UserProposalCard proposal={proposal} userId={userId} />}
    </ProposalList>
  );
}

function UserProposalCard({ proposal, userId }: { proposal: UserProposal; userId: string | null }) {
  const t = useT();
  const locale = useLocale();
  const [busy, setBusy] = useState<ProposalVote | "cancel" | null>(null);
  const [error, setError] = useState<unknown>(null);

  const settled = isSettled(proposal.state, proposal.decided_at);
  const standing = standingInProposal(proposal, userId);
  const tally = proposalTally(proposal);
  const peers = proposal.peers ?? [];
  const kind = proposal.kind;

  const act = async (what: ProposalVote | "cancel") => {
    if (busy) return;
    setBusy(what);
    setError(null);
    try {
      if (what === "cancel") await cancelUserProposal(proposal.id);
      else await voteOnUserProposal(proposal.id, what);
    } catch (cause) {
      setError(cause);
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-start justify-between gap-x-3 gap-y-1.5">
        <div className="flex min-w-0 flex-col gap-1">
          {/* The kind leads, because it is what the reader is being asked to agree to and
              the vote words below deliberately do not say it. */}
          <p className="text-base font-semibold text-foreground">{proposalKindLabel(kind, t)}</p>
          <p className="truncate text-sm text-foreground">{t("consilium.proposals.subject")}: {proposal.subject_email || proposal.subject_user_id}</p>
          <p className="text-sm text-muted-foreground">
            {t("consilium.proposals.openedBy", { initiator: proposal.initiator_email, at: formatMoment(proposal.created_at, locale) })}
          </p>
        </div>
        <Badge variant="outline" className={cn("shrink-0", stateTone(proposal.state))}>
          {stateLabel(proposal.state, t)}
        </Badge>
      </div>

      <div className="flex flex-col gap-5">
        <div className="flex flex-col gap-1.5">
          <span className="text-xs font-medium uppercase tracking-wider text-muted-foreground">{t("consilium.removal.reason")}</span>
          <p className="whitespace-pre-line rounded-lg bg-main-surface px-3.5 py-3 text-sm leading-relaxed text-foreground">
            {proposal.reason?.trim() || t("consilium.removal.noReason")}
          </p>
        </div>

        <div className="flex flex-col gap-2.5">
          <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
            <span className="text-sm font-medium tabular-nums text-foreground">
              {t("consilium.proposals.tally", { n: tally.forVotes, threshold: tally.threshold })}
            </span>
            <span className="text-xs tabular-nums text-muted-foreground">
              {t("consilium.proposals.tallyDetail", { against: tally.againstVotes, waiting: tally.waiting })}
            </span>
          </div>

          {/* A bar, unlike the two consilia above — and it is honest here for the reason it
              would not be there. Unanimity has no meaningful midpoint: one dissent ends it,
              so a bar at 3/4 would be showing progress towards something already lost. A
              majority does have one, and the denominator is the FROZEN threshold rather
              than the head count. */}
          <Progress value={tally.threshold === 0 ? 0 : Math.min(100, (tally.forVotes / tally.threshold) * 100)} />

          {!tally.stillReachable && <p className="text-xs text-destructive">{t("consilium.proposals.unreachable")}</p>}
          <p className="text-xs leading-relaxed text-muted-foreground">{t("consilium.proposals.majorityNote")}</p>

          <ItemGroup>
            {peers.map((peer, i) => (
              <Fragment key={peer.user_id}>
                {i > 0 && <ItemSeparator />}
                <Item size="sm" className="px-0">
                  <ItemContent className="min-w-0 gap-0.5">
                    <ItemTitle className="block w-auto truncate font-medium">{peer.email}</ItemTitle>
                  </ItemContent>
                  <div className={cn("shrink-0 text-xs font-semibold", proposalVoteTone(peer.vote))}>{proposalPeerLabel(peer.vote, t)}</div>
                </Item>
              </Fragment>
            ))}
          </ItemGroup>
        </div>

        {!settled && (
          <p className="text-xs tabular-nums text-muted-foreground">
            {t("consilium.proposals.expires", { at: formatMoment(proposal.expires_at, locale), left: expiresIn(proposal.expires_at, t) })}
          </p>
        )}

        {error !== null && <ResourceError message={errorMessage(error, t)} />}

        {!settled && (
          <>
            <Separator />
            {standing.role === "peer" && standing.vote === null ? (
              // Two equal-weight controls told apart by colour, as on a removal: the reader
              // is being asked to judge, and making one button heavier would be the design
              // taking a side. The destructive tone follows the ACT, not the direction —
              // which is why it is resolved per kind: `for` blocks someone on a suspension
              // and releases them on a reinstatement.
              <div className="flex flex-col gap-2.5 sm:flex-row">
                <Button
                  variant="outline"
                  className={cn("sm:flex-1", kind === "suspension" && "border-destructive/40 text-destructive hover:bg-destructive/10 hover:text-destructive")}
                  disabled={busy !== null}
                  onClick={() => void act("for")}
                >
                  {busy === "for" && <Loader2 className="size-4 animate-spin" />}
                  {proposalVoteLabel(kind, "for", t)}
                </Button>
                <Button variant="outline" className="sm:flex-1" disabled={busy !== null} onClick={() => void act("against")}>
                  {busy === "against" && <Loader2 className="size-4 animate-spin" />}
                  {proposalVoteLabel(kind, "against", t)}
                </Button>
              </div>
            ) : (
              <p className="text-sm leading-relaxed text-muted-foreground">
                <WhyNoVote standing={standing} kind={kind} />
              </p>
            )}

            {standing.role === "initiator" && (
              <Button variant="ghost" size="sm" className="self-start" disabled={busy !== null} onClick={() => void act("cancel")}>
                {busy === "cancel" && <Loader2 className="size-4 animate-spin" />}
                {t("consilium.proposals.withdraw")}
              </Button>
            )}
          </>
        )}
      </div>
    </div>
  );
}

/** Why this reader has no buttons — always stated, never left as an empty space where a
 *  control should be. Each case is a different fact and none of them is a refusal. */
function WhyNoVote({ standing, kind }: { standing: ProposalStanding; kind: string }) {
  const t = useT();
  if (standing.role === "initiator") return <>{t("consilium.proposals.youInitiated")}</>;
  if (standing.role === "subject") return <>{t("consilium.proposals.youAreSubject")}</>;
  if (standing.role === "peer" && standing.vote !== null) {
    return <>{t("consilium.proposals.youVoted", { vote: proposalVoteLabel(kind, standing.vote, t) })}</>;
  }
  return <>{t("consilium.proposals.bystander")}</>;
}

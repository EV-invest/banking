"use client";

// The owners' room: who the owners are, what is currently being decided, and the two things
// an owner can start from here (proposing a removal, and standing down).
//
// Everything on this page is read from the server and nothing is computed from a socket
// frame. The stream says only that a revision moved; the cache re-reads, and these
// components render the answer. That is why there is no optimistic update anywhere in this
// file — a tally is a fact about a decision that moves money or ends an ownership, and the
// only place it is ever computed is inside one Postgres transaction with the request row
// locked (docs/CONSILIUM.md, policy 14, 21).

import { Banknote, Loader2, ShieldAlert, Users } from "lucide-react";
import { Fragment, useState, type CSSProperties } from "react";

import { useLocale, useT } from "@evinvest/i18n/react";
import {
  Badge,
  Button,
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
  Input,
  Item,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemMedia,
  ItemSeparator,
  ItemTitle,
  Label,
  Progress,
  Separator,
  Skeleton,
} from "@evinvest/uikit";

import { useConsiliumStream, type StreamStatus } from "@/entities/governance/model/consilium-socket";
import { cancelConsilium, consiliumResource, ownersResource, removalsResource, resignOwnership } from "@/entities/governance/model/governance-resource";
import { profileResource } from "@/entities/user/model/profile-resource";
import type { Consilium, Owner } from "@/shared/contracts/governance";
import { RequestError, errorMessage } from "@/shared/lib/api-client";
import { cn } from "@/shared/lib/cn";
import { expiresIn, formatDay, formatMoment } from "@/shared/lib/datetime";
import { hashPrefix } from "@/shared/lib/hash";
import { initialsOf } from "@/shared/lib/identity";
import { formatExactUsdt } from "@/shared/lib/money";
import { networkLabel } from "@/shared/lib/rail";
import { useResource } from "@/shared/lib/resource";
import { Link } from "@/shared/ui/cabinet-link";
import { SECTION_STAGGER, Settled, Stagger, StaggerItem } from "@/shared/ui/motion";
import { ResourceError } from "@/shared/ui/resource-error";
import { EMPTY_BOX, isSettled, stateLabel, stateTone } from "@/views/consilium/lib/format";
import { ProposeRemoval, RemovalList } from "@/views/consilium/ui/removals";

const isForbidden = (error: unknown): boolean => error instanceof RequestError && error.status === 403;

export function ConsiliumView() {
  const t = useT();

  const owners = useResource(ownersResource);
  const removals = useResource(removalsResource);
  const consilia = useResource(consiliumResource);
  const profile = useResource(profileResource);

  // Decided before the stream is acquired, not after: a non-owner who opens this URL would
  // otherwise hold a websocket and a 20-second poll against three endpoints that answer 403
  // forever, for a page they are about to be told they cannot see.
  const forbidden = [owners.error, removals.error, consilia.error].some(isForbidden);
  const stream = useConsiliumStream(!forbidden);

  if (forbidden) {
    return (
      <div className="px-4 py-10 lg:px-8">
        <Empty className="mx-auto max-w-160 rounded-xl border border-border bg-card p-6 md:p-8">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <ShieldAlert />
            </EmptyMedia>
            <EmptyTitle>{t("consilium.forbidden.title")}</EmptyTitle>
            <EmptyDescription>{t("consilium.forbidden.body")}</EmptyDescription>
          </EmptyHeader>
          <EmptyContent>
            <Button asChild variant="outline">
              <Link href="/">{t("status.backHome")}</Link>
            </Button>
          </EmptyContent>
        </Empty>
      </div>
    );
  }

  const userId = profile.data?.user_id ?? null;
  const myEmail = profile.data?.email ?? "";
  const roster = owners.data?.items ?? [];
  const openRemovals = (removals.data?.items ?? []).filter((r) => !isSettled(r.state, r.decided_at));
  const items = consilia.data?.items ?? [];
  const openPayouts = items.filter((c) => !isSettled(c.state, c.decided_at));
  const pastPayouts = items.filter((c) => isSettled(c.state, c.decided_at));

  // One derivation per read, and each is only an error when there is nothing to show in its
  // place — a stale roster beside a "couldn't refresh" line beats a blank column. Kept
  // separate on purpose: folding them into one page-level banner is what let a failed
  // consilium read fall through to "No payout is open", which is the one thing this page
  // must never say when it does not actually know.
  const rosterError = owners.data || !owners.error ? null : owners.error;
  const payoutError = consilia.data || !consilia.error ? null : consilia.error;
  const removalError = removals.data || !removals.error ? null : removals.error;

  return (
    <Stagger
      step={SECTION_STAGGER}
      className="grid grid-cols-1 gap-4 px-4 pb-6 pt-5 lg:gap-6 lg:px-8 lg:pb-7 lg:pt-6 xl:grid-cols-(--consilium-columns) xl:items-start"
      style={{ "--consilium-columns": "minmax(0, 1fr) 360px" } as CSSProperties}
    >
      <StaggerItem className="flex flex-wrap items-start justify-between gap-3 xl:col-span-2">
        <div className="flex min-w-0 flex-col gap-1">
          <h1 className="text-2xl font-semibold leading-tight text-foreground">{t("consilium.title")}</h1>
          <p className="text-sm text-muted-foreground">{t("consilium.sub")}</p>
        </div>
        <StreamChip status={stream.status} />
      </StaggerItem>

      {owners.data?.below_payout_floor && (
        // Not styled as an error: nothing has failed. It is a standing fact about the fund
        // that changes what it can do, and it is the first thing an owner should know.
        // Body copy is `text-foreground`, not muted — muted on a tinted ground is the
        // contrast failure AGENTS.md calls out by name.
        <StaggerItem className="flex items-start gap-3 rounded-xl border border-main-accent-t3/40 bg-main-accent-t3/10 px-4 py-3.5 xl:col-span-2">
          <ShieldAlert className="mt-0.5 size-4 shrink-0 text-main-accent-t3" />
          <div className="flex min-w-0 flex-col gap-1">
            <p className="text-sm font-semibold text-foreground">{t("consilium.floor.title")}</p>
            <p className="text-sm leading-relaxed text-foreground">{t("consilium.floor.body", { n: roster.length })}</p>
          </div>
        </StaggerItem>
      )}

      <StaggerItem className="flex flex-col gap-4 lg:gap-6 xl:col-start-1 xl:row-start-3">
        <PayoutSection open={openPayouts} past={pastPayouts} loading={consilia.isLoading} error={payoutError} onRetry={() => void consilia.refresh()} retrying={consilia.isValidating} />
        <RemovalList
          removals={openRemovals}
          userId={userId}
          loading={removals.isLoading}
          error={removalError}
          onRetry={() => void removals.refresh()}
          retrying={removals.isValidating}
        />
        {/* Held back until the roster has actually arrived: rendered against an empty list
            it would flash "There is nobody to propose" before the owners land. */}
        {!owners.isLoading && <ProposeRemoval owners={roster} userId={userId} />}
      </StaggerItem>

      <StaggerItem className="flex flex-col gap-4 lg:gap-6 xl:col-start-2 xl:row-start-3">
        <Roster owners={roster} loading={owners.isLoading} userId={userId} error={rosterError} onRetry={() => void owners.refresh()} retrying={owners.isValidating} />
        <ResignCard email={myEmail} loadingProfile={profile.isLoading} />
      </StaggerItem>
    </Stagger>
  );
}

/**
 * How the page is being kept current — quiet, and honest about the difference.
 *
 * It says "reconnecting" rather than hiding the fact, because the alternative is a page
 * that looks live and is not. The room stays correct either way: the fallback poll is
 * running behind this label (see `entities/governance/model/consilium-socket.ts`).
 */
function StreamChip({ status }: { status: StreamStatus }) {
  const t = useT();
  if (status === "idle") return null;
  const live = status === "live";
  return (
    <Badge variant="outline" className="gap-1.5 rounded-full font-medium text-muted-foreground">
      <span
        className={cn(
          "size-1.5 rounded-full",
          live ? "bg-main-accent-t2" : status === "paused" ? "bg-muted-foreground" : "animate-pulse bg-main-accent-t3",
        )}
      />
      {t(live ? "consilium.stream.live" : status === "paused" ? "consilium.stream.paused" : "consilium.stream.reconnecting")}
    </Badge>
  );
}

function Roster({
  owners,
  loading,
  userId,
  error,
  onRetry,
  retrying,
}: {
  owners: Owner[];
  loading: boolean;
  userId: string | null;
  error: unknown;
  onRetry: () => void;
  retrying: boolean;
}) {
  const t = useT();
  const locale = useLocale();
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t("consilium.roster.title")}</CardTitle>
        <CardAction className="text-xs font-medium tabular-nums text-muted-foreground">
          {t("consilium.roster.count", { n: owners.length })}
        </CardAction>
      </CardHeader>
      <CardContent>
        <Settled loading={loading} skeleton={<Skeleton className="h-40 w-full" />}>
          {loading ? null : error ? (
            <ResourceError error={error} onRetry={onRetry} retrying={retrying} />
          ) : owners.length === 0 ? (
            <Empty className={EMPTY_BOX}>
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <Users />
                </EmptyMedia>
                <EmptyTitle>{t("consilium.roster.emptyTitle")}</EmptyTitle>
                <EmptyDescription>{t("consilium.roster.emptyBody")}</EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <ItemGroup>
              {owners.map((owner, i) => (
                <Fragment key={owner.user_id}>
                  {i > 0 && <ItemSeparator />}
                  <Item size="sm" className="px-0">
                    <ItemMedia>
                      <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-main-accent-t1/15 text-xs font-semibold text-main-accent-t1">
                        {initialsOf(owner.email)}
                      </span>
                    </ItemMedia>
                    <ItemContent className="min-w-0 gap-0.5">
                      <ItemTitle className="block w-auto truncate font-medium">
                        {owner.display_name || owner.email}
                        {owner.user_id === userId && (
                          <span className="ml-1.5 text-xs font-normal text-muted-foreground">{t("consilium.roster.you")}</span>
                        )}
                      </ItemTitle>
                      <ItemDescription className="truncate text-xs tabular-nums">
                        {t("consilium.roster.since", { at: formatDay(owner.owner_since, locale) })}
                      </ItemDescription>
                    </ItemContent>
                  </Item>
                </Fragment>
              ))}
            </ItemGroup>
          )}
        </Settled>
      </CardContent>
    </Card>
  );
}

function PayoutSection({
  open,
  past,
  loading,
  error,
  onRetry,
  retrying,
}: {
  open: Consilium[];
  past: Consilium[];
  loading: boolean;
  error: unknown;
  onRetry: () => void;
  retrying: boolean;
}) {
  const t = useT();
  const locale = useLocale();
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t("consilium.payout.title")}</CardTitle>
        <CardDescription>{t("consilium.payout.sub")}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-5">
        <Settled loading={loading} skeleton={<Skeleton className="h-40 w-full" />}>
          {loading ? null : error ? (
            // In place of the empty state, never beside it. "No payout is open" is a claim
            // about the fund, and a read that failed is not entitled to make it.
            <ResourceError error={error} onRetry={onRetry} retrying={retrying} />
          ) : open.length === 0 ? (
            <Empty className={EMPTY_BOX}>
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <Banknote />
                </EmptyMedia>
                <EmptyTitle>{t("consilium.payout.emptyTitle")}</EmptyTitle>
                <EmptyDescription>{t("consilium.payout.emptyBody")}</EmptyDescription>
              </EmptyHeader>
              <EmptyContent>
                <Button asChild variant="outline">
                  <Link href="/admin/revenue">{t("consilium.payout.emptyAction")}</Link>
                </Button>
              </EmptyContent>
            </Empty>
          ) : (
            <div className="flex flex-col gap-5">
              {open.map((consilium) => (
                <OpenPayout key={consilium.id} consilium={consilium} />
              ))}
            </div>
          )}
        </Settled>

        {past.length > 0 && !error && (
          <>
            <Separator />
            <div className="flex flex-col gap-2">
              {/* Nothing is deleted: a rejected, expired or failed request stays readable,
                  because the ledger and the governance record have to reconcile after the
                  fact (docs/CONSILIUM.md § Audit). */}
              <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">{t("consilium.payout.past")}</p>
              <ItemGroup>
                {past.map((consilium, i) => (
                  <Fragment key={consilium.id}>
                    {i > 0 && <ItemSeparator />}
                    <Item size="sm" className="px-0">
                      <ItemContent className="min-w-0 gap-0.5">
                        <ItemTitle className="block w-auto truncate font-medium tabular-nums">
                          {formatExactUsdt(consilium.revenue_payout?.amount)} USDT · {networkLabel(consilium.revenue_payout?.network)}
                        </ItemTitle>
                        <ItemDescription className="truncate text-xs tabular-nums">
                          {formatMoment(consilium.decided_at ?? consilium.created_at, locale)}
                        </ItemDescription>
                      </ItemContent>
                      <Badge variant="outline" className={cn("shrink-0", stateTone(consilium.state))}>
                        {stateLabel(consilium.state, t)}
                      </Badge>
                    </Item>
                  </Fragment>
                ))}
              </ItemGroup>
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}

function OpenPayout({ consilium }: { consilium: Consilium }) {
  const t = useT();
  const locale = useLocale();
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<unknown>(null);

  const payout = consilium.revenue_payout;
  const threshold = consilium.threshold ?? 0;
  const approvals = consilium.approvals ?? 0;
  const progress = threshold > 0 ? Math.min(100, Math.round((approvals / threshold) * 100)) : 0;

  const cancel = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await cancelConsilium(consilium.id);
      setConfirming(false);
    } catch (cause) {
      setError(cause);
    } finally {
      setBusy(false);
    }
  };

  // No inner card: policy 16 allows at most one open payout at a time, so a second frame
  // inside the section's own Card would be chrome around a single child.
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
        <p className="text-2xl font-semibold leading-none tabular-nums text-foreground">
          {formatExactUsdt(payout?.amount)}
          <span className="ml-2 text-sm font-medium text-muted-foreground">USDT</span>
        </p>
        <Badge variant="outline" className={stateTone(consilium.state)}>
          {stateLabel(consilium.state, t)}
        </Badge>
      </div>

      {/* Full, monospace, wrapped rather than truncated — the same rule as the approval
          email and the approval page. An owner who checks the address here and approves it
          there must be looking at the same characters (policy 13). */}
      <p className="break-all rounded-lg border border-border bg-main-surface px-3 py-2.5 font-mono-tech text-xs leading-relaxed text-foreground">
        {payout?.address || "—"}
      </p>

      <div className="flex flex-col gap-1.5 text-xs text-muted-foreground">
        <span className="tabular-nums">{t("consilium.payout.network", { network: networkLabel(payout?.network) })}</span>
        <span className="font-mono-tech">{t("consilium.payout.fingerprint", { hash: hashPrefix(consilium.payload_hash) })}</span>
        <span>{t("consilium.payout.openedBy", { initiator: consilium.initiator_email })}</span>
        <span className="tabular-nums">
          {t("consilium.payout.expires", { at: formatMoment(consilium.expires_at, locale), left: expiresIn(consilium.expires_at, t) })}
        </span>
      </div>

      <div className="flex flex-col gap-2">
        <div className="flex items-baseline justify-between gap-3">
          <span className="text-sm font-medium tabular-nums text-foreground">
            {t("consilium.payout.tally", { approvals, threshold })}
          </span>
          <span className="text-xs tabular-nums text-muted-foreground">{t("consilium.payout.owners", { n: consilium.owner_count ?? 0 })}</span>
        </div>
        <Progress value={progress} className="h-1.5" aria-hidden />
        <p className="text-xs text-muted-foreground">{t("consilium.payout.voteByEmail")}</p>
      </div>

      {error !== null && <ResourceError message={errorMessage(error, t)} />}

      {confirming ? (
        // Cancelling voids every approval collected so far — votes are not carried over
        // when a request is reopened (policy 12), so this is not the reversible click its
        // single ghost button made it look like.
        <div className="flex flex-col gap-3 rounded-lg border border-destructive/40 bg-destructive/10 p-3.5">
          <p className="text-sm leading-relaxed text-foreground">{t("consilium.payout.cancelWarning", { approvals })}</p>
          <div className="flex flex-col gap-2.5 sm:flex-row">
            <Button variant="destructive" size="sm" disabled={busy} onClick={() => void cancel()}>
              {busy && <Loader2 className="size-4 animate-spin" />}
              {t("consilium.payout.cancelConfirm")}
            </Button>
            <Button variant="ghost" size="sm" disabled={busy} onClick={() => setConfirming(false)}>
              {t("ui.cancel")}
            </Button>
          </div>
        </div>
      ) : (
        <Button variant="ghost" size="sm" className="self-start" onClick={() => setConfirming(true)}>
          {t("consilium.payout.cancel")}
        </Button>
      )}
    </div>
  );
}

/**
 * Stand down as an owner.
 *
 * No consilium — nobody has to agree to let you leave — but the same floor applies, and the
 * confirmation is typing your own address rather than pressing a second button. That is the
 * `confirm_email` the BFF asks for, and its only job is to make this impossible to do by
 * accident, which is why what the reader typed is what gets sent.
 */
function ResignCard({ email, loadingProfile }: { email: string; loadingProfile: boolean }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [done, setDone] = useState(false);

  const matches = email.length > 0 && typed.trim().toLowerCase() === email.toLowerCase();

  const submit = async () => {
    if (busy || !matches) return;
    setBusy(true);
    setError(null);
    try {
      await resignOwnership(typed.trim());
      setDone(true);
      setOpen(false);
      setTyped("");
    } catch (cause) {
      setError(cause);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t("consilium.resign.title")}</CardTitle>
        <CardDescription className="text-balance">{t("consilium.resign.sub")}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3.5">
        {done ? (
          <p className="text-sm text-main-accent-t2" role="status">
            {t("consilium.resign.done")}
          </p>
        ) : loadingProfile ? (
          <Skeleton className="h-9 w-40" />
        ) : email.length === 0 ? (
          // Without an email there is nothing to type and nothing to compare, so the button
          // would sit permanently disabled with no way to find out why.
          <p className="text-sm text-muted-foreground">{t("consilium.resign.unavailable")}</p>
        ) : open ? (
          <>
            <p className="text-sm leading-relaxed text-muted-foreground">{t("consilium.resign.warning")}</p>
            <p className="text-xs text-muted-foreground">{t("consilium.resign.floorWarning")}</p>
            <div className="flex flex-col gap-2">
              <Label htmlFor="resign-confirm">{t("consilium.resign.confirmLabel", { email })}</Label>
              <Input
                id="resign-confirm"
                value={typed}
                onChange={(e) => setTyped(e.target.value)}
                autoComplete="off"
                autoCapitalize="none"
                inputMode="email"
                spellCheck={false}
                placeholder={email}
              />
            </div>
            {error !== null && <ResourceError message={errorMessage(error, t)} />}
            <div className="flex flex-col gap-2.5 sm:flex-row">
              <Button variant="destructive" className="sm:flex-1" disabled={!matches || busy} onClick={() => void submit()}>
                {busy && <Loader2 className="size-4 animate-spin" />}
                {t("consilium.resign.confirm")}
              </Button>
              <Button
                variant="ghost"
                disabled={busy}
                onClick={() => {
                  setOpen(false);
                  setTyped("");
                  setError(null);
                }}
              >
                {t("ui.cancel")}
              </Button>
            </div>
          </>
        ) : (
          <Button variant="outline" className="self-start" onClick={() => setOpen(true)}>
            {t("consilium.resign.start")}
          </Button>
        )}
      </CardContent>
    </Card>
  );
}

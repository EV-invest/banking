"use client";

import { Inbox, KeyRound, RefreshCw, TriangleAlert } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Card, CardContent, Empty, EmptyHeader, EmptyMedia, EmptyTitle, Skeleton, Spinner, Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@evinvest/uikit";

import { unparkEvent } from "@/entities/admin/api/admin-client";
import { parkedEventsResource } from "@/entities/admin/model/admin-resource";
import { errorMessage, RequestError } from "@/shared/lib/api-client";
import { useResource } from "@/shared/lib/resource";
import { TipAnchor } from "@/shared/tips";
import { ago } from "@/views/admin/lib/format";
import { TABLE_HEAD } from "@/views/admin/lib/table";
import { isDeadKeyPark } from "@/views/admin/outbox/lib/dead-key";
import { StaggerItem } from "@/shared/ui/motion";
import type { ParkedEvent } from "@/shared/contracts/admin";
import { AdminHeader, AdminScreen } from "@/views/admin/ui/shell";

// The one operator surface that cannot move to Grafana: fleet health and the KPIs
// are read-only and live on a dashboard now, but unparking a stuck outbox row is an
// action, and an action needs a place in the cabinet that the BFF authorizes.
export function OutboxView() {
  const t = useT();
  const [refreshing, setRefreshing] = useState(false);
  const [unparkError, setUnparkError] = useState<string | null>(null);
  const [unparking, setUnparking] = useState<string | null>(null);
  // Rows whose unpark POST succeeded but whose re-read failed — still listed, but they must
  // not offer a second unpark. A successful re-read drops them from the list.
  const [unparked, setUnparked] = useState<ReadonlySet<string>>(new Set());
  const [refetchError, setRefetchError] = useState<string | null>(null);

  // Cached, so returning to Outbox shows the list it last held and settles it behind.
  const parkedRead = useResource(parkedEventsResource);
  const parked = parkedRead.data ? (parkedRead.data.events ?? []) : null;
  // Best-effort: a money plane that isn't connected renders as a muted hint, not an error
  // banner — there is nothing else on this screen for a banner to shout over.
  const parkedHint = parked || !parkedRead.error ? null : errorMessage(parkedRead.error, t);

  const load = () => {
    setRefreshing(true);
    setRefetchError(null);
    void parkedRead.refresh().then(
      () => {
        setUnparked(new Set());
        setRefreshing(false);
      },
      () => setRefreshing(false),
    );
  };

  const unpark = async (seq: string) => {
    setUnparking(seq);
    setUnparkError(null);
    setRefetchError(null);
    try {
      const { ok } = await unparkEvent(seq);
      // A `RequestError` rather than a bare `Error`: the transport call succeeded, so the
      // refusal has to carry its own catalogue key to reach the reader in their language.
      if (!ok) throw new RequestError("the hub declined the unpark", 200, "err.unparkDeclined");
    } catch (e) {
      setUnparkError(errorMessage(e, t));
      setUnparking(null);
      return;
    }
    // The POST succeeded — mark the row unparked before the re-read so a failed re-read
    // can't leave an enabled Unpark button on an already-unparked event.
    setUnparked((prev) => new Set(prev).add(seq));
    try {
      await parkedRead.refresh();
      setUnparked(new Set());
    } catch (e) {
      setRefetchError(errorMessage(e, t));
    } finally {
      setUnparking(null);
    }
  };

  return (
    <AdminScreen className="space-y-8">
      <AdminHeader
        eyebrow={t("admin.eyebrow.administer")}
        title={t("nav.outbox")}
        subtitle={t("admin.outbox.subtitle")}
        action={
          <Button type="button" variant="outline" size="sm" disabled={refreshing} onClick={load}>
            <RefreshCw className={refreshing ? "size-4 animate-spin" : "size-4"} /> {t("ui.refresh")}
          </Button>
        }
      />

      <StaggerItem as={Card}>
        <CardContent className="space-y-4 py-5">
          <div>
            <h2 className="text-base font-semibold">{t("admin.outbox.parkedEvents")}</h2>
            <p className="text-xs text-ink-soft">{t("admin.outbox.parkedEventsSub")}</p>
          </div>
          {unparkError && (
            <p className="flex items-center gap-2 text-xs text-accent-error">
              <TriangleAlert className="size-3.5" /> {unparkError}
            </p>
          )}
          {refetchError && (
            <p className="flex items-center gap-2 text-xs text-accent-warn">
              <TriangleAlert className="size-3.5" /> {t("admin.outbox.unparkRefetchFailed", { error: refetchError })}
            </p>
          )}
          {!parked ? (
            <Skeleton className="h-16 w-full" />
          ) : parkedHint ? (
            <p className="text-sm text-ink-soft">{parkedHint}</p>
          ) : parked.length === 0 ? (
            <Empty className="border">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <Inbox />
                </EmptyMedia>
                <EmptyTitle>{t("admin.outbox.noParkedEvents")}</EmptyTitle>
              </EmptyHeader>
            </Empty>
          ) : (
            <ParkedTable parked={parked} unparked={unparked} unparking={unparking} onUnpark={(seq) => void unpark(seq)} />
          )}
        </CardContent>
      </StaggerItem>
    </AdminScreen>
  );
}

function ParkedTable({
  parked,
  unparked,
  unparking,
  onUnpark,
}: {
  parked: readonly ParkedEvent[];
  unparked: ReadonlySet<string>;
  unparking: string | null;
  onUnpark: (seq: string) => void;
}) {
  const t = useT();
  return (
    <Table>
      <TableHeader>
        {/* i18n-max: 14 per header — auto-layout table; the Reason cell is the one
            that gives width back, and it is already `truncate`d. */}
        <TableRow>
          <TableHead className={TABLE_HEAD}>{t("admin.outbox.col.seq")}</TableHead>
          <TableHead className={TABLE_HEAD}>{t("admin.outbox.col.event")}</TableHead>
          <TableHead className={TABLE_HEAD}>
            <span className="flex items-center gap-1.5">
              {t("admin.outbox.col.reason")}
              <TipAnchor anchor="admin.outbox.parked.reason" />
            </span>
          </TableHead>
          <TableHead className={TABLE_HEAD}>{t("admin.outbox.col.parked")}</TableHead>
          <TableHead className={TABLE_HEAD} />
        </TableRow>
      </TableHeader>
      <TableBody>
        {parked.map((e) => (
          <TableRow key={e.seq}>
            <TableCell className="font-mono-tech text-xs text-ink-soft">{e.seq}</TableCell>
            <TableCell>
              <p className="font-medium">{e.kind}</p>
              <p className="font-mono-tech text-xs text-ink-soft">
                {e.aggregate} · {e.aggregate_id}
              </p>
            </TableCell>
            <TableCell className="text-ink-soft">
              <div className="max-w-70 truncate" title={e.reason}>
                {e.reason || "—"}
              </div>
            </TableCell>
            <TableCell className="text-ink-soft">{ago(e.parked_at, t)}</TableCell>
            <TableCell className="text-right">
              <ParkedActions event={e} unparked={unparked.has(e.seq)} unparking={unparking} onUnpark={() => onUnpark(e.seq)} />
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}

function ParkedActions({
  event,
  unparked,
  unparking,
  onUnpark,
}: {
  event: ParkedEvent;
  unparked: boolean;
  unparking: string | null;
  onUnpark: () => void;
}) {
  const t = useT();
  return (
    <div className="flex items-center justify-end gap-2">
      {/* i18n-max: 12 per badge — three chips and a button share this cell. */}
      {event.compensated && (
        <span className="flex items-center gap-1.5 whitespace-nowrap rounded-full bg-ink/5 px-2 py-0.5 text-xs font-medium text-ink">
          {t("admin.outbox.compensated")}
          <TipAnchor anchor="admin.outbox.parked.compensated" />
        </span>
      )}
      {unparked && (
        <span className="whitespace-nowrap rounded-full bg-positive/15 px-2 py-0.5 text-xs font-medium text-positive">{t("admin.outbox.unparked")}</span>
      )}
      {isDeadKeyPark(event.reason) ? (
        // No button at all: unparking a dead-key row re-parks it on the same refusal, and
        // an operator cannot rotate the key — the hint says who can and what they are waiting on.
        <span className="flex max-w-64 items-start gap-1.5 text-left text-xs text-accent-warn">
          <KeyRound className="mt-0.5 size-3.5 shrink-0" aria-hidden />
          {t("admin.outbox.deadKey")}
        </span>
      ) : (
        <>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={event.compensated || unparked || unparking !== null}
            onClick={onUnpark}
          >
            {unparking === event.seq ? <Spinner className="size-3.5" aria-hidden /> : null}
            {t("admin.outbox.unpark")}
          </Button>
          <TipAnchor anchor="admin.outbox.parked.unpark" />
        </>
      )}
    </div>
  );
}

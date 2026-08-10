"use client";

import { ArrowLeftRight, ChevronRight, ListChecks, TriangleAlert } from "lucide-react";
import Link from "next/link";
import { Fragment, useEffect, useMemo, useState } from "react";

import {
  Alert,
  AlertDescription,
  AlertTitle,
  Badge,
  Button,
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemMedia,
  ItemSeparator,
  ItemTitle,
  Skeleton,
  ToggleGroup,
  ToggleGroupItem,
} from "@evinvest/uikit";

import { fetchAllocations } from "@/entities/fund/api/fund-client";
import { fetchOperations } from "@/entities/operation/api/operation-client";
import type { Allocation, Operation } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import {
  amountTone,
  dayLabel,
  formatUnits,
  formatUsdt,
  isPending,
  KIND_FILTERS,
  kindMeta,
  networkLabel,
  seconds,
  shortAddress,
  stateTone,
  timeLabel,
} from "@/views/operations/lib/format";

// uikit's Empty draws a dashed frame but leaves the border width to the caller, and
// doubles its padding at `md`; these sit inside cards, not on a page of their own.
const EMPTY_BOX = "border md:p-6";

type Filter = "all" | (typeof KIND_FILTERS)[number];

// The activity timeline (Figma `ios/operations` · `android/operations`, and the desktop
// "Recent operations" card's `View all`). Every money movement the user made, in one
// time-ordered feed, served pre-merged by the hub's `ListOperations` read model — the
// four sources have their own list endpoints, but only the hub can interleave them by
// time and still apply one page limit across the result.
//
// Read-only by design: the cancel affordances for a queued withdrawal and a queued
// redemption already live on the surfaces that own those aggregates (`/wallet/activity`
// and `/invest`), and the In progress card links to them rather than growing a third
// copy of the same mutation.
export function OperationsView() {
  const [operations, setOperations] = useState<Operation[] | undefined>(undefined);
  const [truncated, setTruncated] = useState(false);
  const [catalog, setCatalog] = useState<Allocation[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>("all");

  useEffect(() => {
    fetchOperations()
      .then((list) => {
        setOperations(list.operations ?? []);
        setTruncated(list.truncated ?? false);
        setError(null);
      })
      .catch((e: Error) => {
        setOperations([]);
        setError(e.message);
      });
  }, []);

  // Fund slugs are keys, not names. The catalog turns them into the product the investor
  // actually bought; a failed lookup degrades to the slug rather than blanking the row.
  useEffect(() => {
    fetchAllocations()
      .then((list) => setCatalog(list.allocations ?? []))
      .catch(() => setCatalog([]));
  }, []);

  const titleOf = useMemo(() => {
    const byService = new Map(catalog.map((a) => [a.service, a.title]));
    return (service: string | undefined) => (service ? (byService.get(service) ?? service) : "Fund");
  }, [catalog]);

  const loading = operations === undefined;
  const all = operations ?? [];
  // The filter applies to both bands. Lifting a pending row into "In progress" while the
  // filter still let it through below would leave a "Deposits" view showing a withdrawal.
  const visible = filter === "all" ? all : all.filter((o) => o.kind === filter);
  // Each operation appears in exactly one of the two bands. Showing a queued withdrawal
  // in "In progress" *and* again under today's date reads as two withdrawals — which on
  // a money screen is not a cosmetic problem. The lifted rows are the newest anyway, so
  // the timeline loses no chronology by starting below them.
  const pending = visible.filter(isPending);
  const settled = visible.filter((o) => !isPending(o));
  const groups = useMemo(() => groupByDay(settled), [settled]);

  return (
    <div className="px-4 pb-8 pt-6 lg:px-8">
      <header className="mb-6 space-y-1">
        <p className="font-mono-tech text-xs uppercase tracking-widest text-main-accent-t1">Operations</p>
        <h1 className="font-sans text-2xl font-semibold text-foreground">Operations</h1>
        <p className="text-sm text-muted-foreground">Every deposit, withdrawal, subscription and redemption you have made — one timeline.</p>
      </header>

      {error && (
        <Alert variant="destructive" className="mb-6">
          <TriangleAlert className="size-4" />
          <AlertTitle>Couldn&apos;t load your operations</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      {loading ? (
        <div className="space-y-4">
          <Skeleton className="h-10 w-72 rounded-lg" />
          <Skeleton className="h-80 w-full rounded-xl" />
        </div>
      ) : all.length === 0 ? (
        <Card>
          <CardContent>
            <Empty className={EMPTY_BOX}>
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <ListChecks />
                </EmptyMedia>
                <EmptyTitle>No operations yet</EmptyTitle>
                <EmptyDescription>Add funds to your wallet, then subscribe into a fund — every movement appears here from the moment you make it, not once it settles.</EmptyDescription>
              </EmptyHeader>
              <EmptyContent>
                <Button asChild>
                  <Link href="/wallet/deposit">Add funds</Link>
                </Button>
                <Button asChild variant="outline">
                  <Link href="/invest">Browse funds</Link>
                </Button>
              </EmptyContent>
            </Empty>
          </CardContent>
        </Card>
      ) : (
        <div className="space-y-6">
          {/* A filter, not a set of panels: ToggleGroup rather than Tabs, because a
              TabsList whose triggers control no TabPanel leaves every `aria-controls`
              pointing at nothing. `type="single"` fires an empty value when the active
              item is clicked again, which here means "show everything". */}
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            value={filter}
            onValueChange={(value) => setFilter((typeof value === "string" && value ? value : "all") as Filter)}
            aria-label="Filter operations by kind"
            // The group is `w-fit`, so past the viewport it must scroll rather than
            // squeeze — five labels do not fit across 390px at any legible size.
            className="max-w-full overflow-x-auto"
          >
            {/* uikit's items are `flex-1`, which splits the row evenly and lets
                "Redemptions" overflow its own cell into its neighbour. These size to
                their own text instead. */}
            <ToggleGroupItem value="all" className="flex-none px-3">
              All
            </ToggleGroupItem>
            {KIND_FILTERS.map((kind) => (
              <ToggleGroupItem key={kind} value={kind} className="flex-none px-3">
                {kindMeta(kind).label}s
              </ToggleGroupItem>
            ))}
          </ToggleGroup>

          {visible.length === 0 ? (
            <Card>
              <CardContent>
                <Empty className={EMPTY_BOX}>
                  <EmptyHeader>
                    <EmptyMedia variant="icon">
                      <ArrowLeftRight />
                    </EmptyMedia>
                    <EmptyTitle>No {kindMeta(filter).label.toLowerCase()}s yet</EmptyTitle>
                    <EmptyDescription>You have other activity — switch back to All to see it.</EmptyDescription>
                  </EmptyHeader>
                </Empty>
              </CardContent>
            </Card>
          ) : (
            <div className="space-y-6">
              {pending.length > 0 && <InProgress operations={pending} titleOf={titleOf} />}
              {groups.map((group) => (
                <section key={group.label} className="space-y-2">
                  <h2 className="px-1 text-xs font-medium uppercase tracking-wide text-muted-foreground">{group.label}</h2>
                  <Card>
                    <CardContent>
                      <ItemGroup>
                        {group.operations.map((operation, i) => (
                          <Fragment key={rowKey(operation, i)}>
                            {i > 0 && <ItemSeparator />}
                            <Row operation={operation} titleOf={titleOf} />
                          </Fragment>
                        ))}
                      </ItemGroup>
                    </CardContent>
                  </Card>
                </section>
              ))}
            </div>
          )}

          {truncated && <p className="text-xs text-muted-foreground">Showing your most recent operations. Older activity is kept on your statement.</p>}
        </div>
      )}
    </div>
  );
}

// The "In progress" card (Figma `ios/operations`): everything still moving, lifted to the
// top so a queued withdrawal is not buried under a day of settled rows. These rows do not
// repeat in the timeline below — see the split in `OperationsView`.
function InProgress({ operations, titleOf }: { operations: Operation[]; titleOf: (service: string | undefined) => string }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>In progress</CardTitle>
      </CardHeader>
      <CardContent>
        <ItemGroup>
          {operations.map((operation, i) => (
            <Fragment key={rowKey(operation, i)}>
              {i > 0 && <ItemSeparator />}
              <Row operation={operation} titleOf={titleOf} linkToManage />
            </Fragment>
          ))}
        </ItemGroup>
      </CardContent>
    </Card>
  );
}

function Row({ operation, titleOf, linkToManage = false }: { operation: Operation; titleOf: (service: string | undefined) => string; linkToManage?: boolean }) {
  const meta = kindMeta(operation.kind);
  const at = seconds(operation.created_at);
  const manage = linkToManage ? manageHref(operation.kind) : null;

  const body = (
    <>
      <ItemMedia>
        <Badge className={cn("font-semibold", meta.tone)}>{meta.badge}</Badge>
      </ItemMedia>
      <ItemContent className="min-w-0 gap-0.5">
        <ItemTitle className="block w-auto truncate font-semibold">{rowTitle(operation, titleOf)}</ItemTitle>
        <ItemDescription className="line-clamp-1 text-xs">
          {rowSub(operation)}
          {at > 0 && ` · ${timeLabel(at)}`}
        </ItemDescription>
      </ItemContent>
      <ItemActions className="shrink-0 flex-col items-end gap-1">
        <span className={cn("text-sm font-semibold tabular-nums", amountTone(meta.direction))}>{rowAmount(operation)}</span>
        <Badge className={cn("capitalize", stateTone(operation.state))}>{operation.state}</Badge>
      </ItemActions>
      {manage && <ChevronRight className="size-4 shrink-0 text-muted-foreground" aria-hidden />}
    </>
  );

  // A pending row's whole surface is the affordance rather than a trailing "Manage"
  // button: at 390px that button cost enough width to truncate "Withdrawal" itself, and
  // a full-row target is the easier one to hit besides. Item brings its own focus ring
  // through the anchor.
  if (manage) {
    return (
      <Item asChild size="sm" className="px-0 py-3 transition-colors hover:bg-muted/40 lg:py-4">
        <Link href={manage} aria-label={`${rowTitle(operation, titleOf)} — manage`}>
          {body}
        </Link>
      </Item>
    );
  }

  return (
    <Item size="sm" className="px-0 py-3 lg:py-4">
      {body}
    </Item>
  );
}

// The surface that owns this kind's mutations (cancel a queued withdrawal / redemption).
// Deposits and subscriptions have nothing to act on once made.
function manageHref(kind: string | undefined): string | null {
  if (kind === "withdrawal") return "/wallet/activity";
  if (kind === "redemption") return "/invest";
  return null;
}

function rowTitle(operation: Operation, titleOf: (service: string | undefined) => string): string {
  switch (operation.kind) {
    case "deposit":
      return "Deposit";
    case "withdrawal":
      return "Withdrawal";
    case "subscription":
      return `${titleOf(operation.service)} — subscribed`;
    case "redemption":
      return `${titleOf(operation.service)} — redeemed`;
    default:
      return kindMeta(operation.kind).label;
  }
}

function rowSub(operation: Operation): string {
  switch (operation.kind) {
    case "deposit":
      return `${networkLabel(operation.network)} · ${operation.tx_ref ? shortAddress(operation.tx_ref) : "USDT"}`;
    case "withdrawal": {
      const destination = operation.address ? shortAddress(operation.address) : networkLabel(operation.network);
      // What actually ships is the net; the row's figure is the gross debited, so the
      // fee is stated rather than left as an unexplained gap between the two.
      const net = operation.net_amount ? ` · ${formatUsdt(operation.net_amount)} USDT net` : "";
      return `${networkLabel(operation.network)} · ${destination}${net}`;
    }
    case "subscription":
    case "redemption":
      return `${formatUnits(operation.units)} units`;
    default:
      return "";
  }
}

function rowAmount(operation: Operation): string {
  // A redemption is priced at settle, so a queued one genuinely has no cash figure —
  // rendered as an em dash rather than a zero that would read as "you got nothing".
  if (!operation.amount) return "—";
  const { direction } = kindMeta(operation.kind);
  const sign = direction === "in" ? "+" : direction === "out" ? "−" : "";
  return `${sign}${formatUsdt(operation.amount)} USDT`;
}

// `id` is unique per kind (a UUID, or a deposit's tx_ref) but nothing guarantees it across
// kinds, and a multi-output deposit tx can repeat its ref — so the key carries the kind
// and the row's position too.
function rowKey(operation: Operation, index: number): string {
  return `${operation.kind ?? ""}-${operation.id ?? ""}-${index}`;
}

interface DayGroup {
  label: string;
  operations: Operation[];
}

// The feed arrives newest-first from the hub; this only inserts the day headings, so the
// runs stay in the order the hub sorted them.
function groupByDay(operations: Operation[]): DayGroup[] {
  const now = new Date();
  const groups: DayGroup[] = [];
  for (const operation of operations) {
    const label = dayLabel(seconds(operation.created_at), now);
    const last = groups.at(-1);
    if (last?.label === label) last.operations.push(operation);
    else groups.push({ label, operations: [operation] });
  }
  return groups;
}

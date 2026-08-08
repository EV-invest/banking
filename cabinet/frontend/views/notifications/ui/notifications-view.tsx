"use client";

import { Bell } from "lucide-react";
import Link from "next/link";
import { useEffect, useState } from "react";

import { fetchNotifications, markRead } from "@/entities/notification/api/notification-client";
import { publishUnreadCount } from "@/entities/notification/model/notification-store";
import type { Notification } from "@/shared/contracts/notifications";
import { isUnread, toDate } from "@/shared/contracts/notifications";
import { cn } from "@/shared/lib/cn";

const CARD = "rounded-xl border border-border bg-main-card";
// Every control on this screen is hand-written rather than a uikit Button, so the keyboard
// focus ring has to be written out — once, here, so the four of them cannot drift apart.
const FOCUS = "outline-none focus-visible:ring-2 focus-visible:ring-ring";
// "Mark all read" and "Load older" are the same control in two places.
const GHOST_BUTTON = `rounded-lg border border-border/60 px-4 py-2 text-sm font-medium text-foreground transition-colors hover:bg-foreground/5 disabled:opacity-40 ${FOCUS}`;
// Rows are dense, so the inset is wider than the vertical rhythm.
const ROW_PAD = "px-5.5 py-4.5";
// A row spans the full width of a card that clips its overflow, so an outset ring would be
// shaved off on both sides — this one draws inside the row instead.
const ROW = "block outline-none transition-colors hover:bg-foreground/5 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring";

type Filter = "all" | "unread";

/**
 * The inbox. Reads are cursor-paginated and the filter round-trips to the server
 * rather than filtering client-side, so "Unread" is accurate across pages instead of
 * only within the ones already loaded.
 *
 * An empty list is ambiguous on its own — it means either "nothing has happened yet"
 * or "you switched the in-app channel off", and those want different copy. The
 * settings link in the empty state covers the second case without us having to fetch
 * settings here just to tell them apart.
 */
export function NotificationsView() {
  const [items, setItems] = useState<Notification[] | null>(null);
  const [cursor, setCursor] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [unread, setUnread] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // A reload is expressed as a key bump rather than a callable, so every setState
  // lands in a promise callback. The react-hooks lint rejects an effect that calls a
  // function which updates state synchronously, and it is right to: that is a
  // cascading render.
  const [reloadKey, setReloadKey] = useState(0);

  useEffect(() => {
    let alive = true;
    fetchNotifications({ filter })
      .then((page) => {
        if (!alive) return;
        setError(null);
        setItems(page.notifications);
        setCursor(page.next_cursor);
        setUnread(page.unread_count);
        publishUnreadCount(page.unread_count);
      })
      .catch((e: unknown) => {
        if (!alive) return;
        setError(e instanceof Error ? e.message : "could not load notifications");
        setItems([]);
      });
    return () => {
      alive = false;
    };
  }, [filter, reloadKey]);

  async function loadMore() {
    if (!cursor || busy) return;
    setBusy(true);
    try {
      const page = await fetchNotifications({ filter, cursor });
      setItems((prev) => [...(prev ?? []), ...page.notifications]);
      setCursor(page.next_cursor);
    } catch (e) {
      setError(e instanceof Error ? e.message : "could not load more");
    } finally {
      setBusy(false);
    }
  }

  async function markAll() {
    if (busy || unread === 0) return;
    setBusy(true);
    try {
      const res = await markRead();
      setUnread(res.unread_count);
      publishUnreadCount(res.unread_count);
      // Re-read rather than patching locally: under the "unread" filter the rows that
      // just changed should disappear, which a local patch would not do.
      setReloadKey((k) => k + 1);
    } catch (e) {
      setError(e instanceof Error ? e.message : "could not mark as read");
    } finally {
      setBusy(false);
    }
  }

  async function open(n: Notification) {
    if (!isUnread(n)) return;
    // Optimistic: reading is not a destructive act, and a failed mark simply shows up
    // again on the next load.
    setItems((prev) => prev?.map((x) => (x.id === n.id ? { ...x, read_at: String(Math.floor(Date.now() / 1000)) } : x)) ?? prev);
    setUnread((u) => Math.max(0, u - 1));
    try {
      const res = await markRead([n.id]);
      publishUnreadCount(res.unread_count);
    } catch {
      /* the next load reconciles */
    }
  }

  return (
    <div className="mx-auto w-full max-w-282 px-4 py-6 sm:px-6 lg:px-8">
      <header className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h1 className="text-3xl font-semibold text-foreground">Notifications</h1>
          <p className="mt-1 text-sm text-muted-foreground">Everything you follow — and how you hear about it</p>
        </div>
        <button type="button" onClick={markAll} disabled={busy || unread === 0} className={GHOST_BUTTON}>
          Mark all read
        </button>
      </header>

      <div className="mt-5 inline-flex gap-0.5 rounded-lg border border-border/60 bg-main-surface p-1">
        {(["all", "unread"] as const).map((f) => (
          <button
            key={f}
            type="button"
            onClick={() => setFilter(f)}
            aria-pressed={filter === f}
            className={cn(
              "rounded-md px-3.5 py-1.5 text-sm transition-colors",
              FOCUS,
              filter === f ? "bg-primary font-semibold text-primary-foreground" : "font-medium text-foreground hover:bg-foreground/5",
            )}
          >
            {f === "all" ? "All" : unread > 0 ? `Unread · ${unread}` : "Unread"}
          </button>
        ))}
      </div>

      {error && (
        <p className="mt-4 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</p>
      )}

      <div className={cn("mt-4 overflow-hidden", CARD)}>
        {items === null ? (
          <ul>
            {[0, 1, 2].map((i) => (
              <li key={i} className={cn("flex items-center gap-3.5", ROW_PAD, i > 0 && "border-t border-border/10")}>
                <div className="size-9 animate-pulse rounded-lg bg-foreground/5" />
                <div className="flex-1 space-y-2">
                  <div className="h-3.5 w-1/3 animate-pulse rounded bg-foreground/5" />
                  <div className="h-3 w-2/3 animate-pulse rounded bg-foreground/5" />
                </div>
              </li>
            ))}
          </ul>
        ) : items.length === 0 ? (
          <EmptyState filter={filter} />
        ) : (
          <ul>
            {items.map((n, i) => (
              <Row key={n.id} n={n} first={i === 0} onOpen={() => void open(n)} />
            ))}
          </ul>
        )}
      </div>

      {cursor && items && items.length > 0 && (
        <div className="mt-4 flex justify-center">
          <button type="button" onClick={loadMore} disabled={busy} className={GHOST_BUTTON}>
            {busy ? "Loading…" : "Load older"}
          </button>
        </div>
      )}
    </div>
  );
}

function Row({ n, first, onOpen }: { n: Notification; first: boolean; onOpen: () => void }) {
  const unread = isUnread(n);
  const body = (
    <div className={cn("flex items-center gap-3.5 text-left", ROW_PAD, unread && "bg-foreground/5")}>
      <span aria-hidden className={cn("size-2 shrink-0 rounded-full", unread ? "bg-main-accent-t1" : "bg-transparent")} />
      <div className="min-w-0 flex-1">
        {/* Read and unread titles share a step on the type scale, so the state is carried
            by weight and colour instead of the 1px that used to separate them. */}
        <p className={cn("truncate text-sm", unread ? "font-semibold text-foreground" : "text-muted-foreground")}>{n.title}</p>
        {n.body && <p className="mt-0.5 line-clamp-2 text-xs text-muted-foreground">{n.body}</p>}
      </div>
      <time className="shrink-0 text-xs tabular-nums text-muted-foreground" dateTime={toDate(n.created_at)?.toISOString()}>
        {formatWhen(n.created_at)}
      </time>
    </div>
  );

  return (
    <li className={cn(!first && "border-t border-border/10")}>
      {n.link ? (
        <Link href={n.link} onClick={onOpen} className={ROW}>
          {body}
        </Link>
      ) : (
        <button type="button" onClick={onOpen} className={cn("w-full", ROW)}>
          {body}
        </button>
      )}
    </li>
  );
}

function EmptyState({ filter }: { filter: Filter }) {
  return (
    <div className="flex flex-col items-center px-10 py-16 text-center">
      <span className="flex size-14 items-center justify-center rounded-xl bg-main-accent-t1/15">
        <Bell className="size-6 text-main-accent-t1" />
      </span>
      <p className="mt-5 text-base font-semibold text-foreground">{filter === "unread" ? "Nothing unread" : "Nothing yet"}</p>
      <p className="mt-2 max-w-108 text-sm text-muted-foreground">
        {filter === "unread"
          ? "You're all caught up."
          : "Follow a fund and its NAV updates, distributions and research notes land here — and in your inbox too, if you want a copy."}
      </p>
      <Link
        href="/settings"
        className={cn("mt-5 rounded-lg border border-border px-5.5 py-2.5 text-sm font-semibold text-foreground transition-colors hover:bg-foreground/5", FOCUS)}
      >
        Notification settings
      </Link>
    </div>
  );
}

/** Relative for the first day, then a plain date — matching how the inbox is scanned. */
function formatWhen(unixSeconds: string): string {
  const d = toDate(unixSeconds);
  if (!d) return "";
  const mins = Math.floor((Date.now() - d.getTime()) / 60_000);
  if (mins < 1) return "now";
  if (mins < 60) return `${mins}m`;
  if (mins < 60 * 24) return `${Math.floor(mins / 60)}h`;
  if (mins < 60 * 24 * 7) return `${Math.floor(mins / (60 * 24))}d`;
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

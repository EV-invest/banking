"use client";

// The cabinet's read cache: one value per endpoint, shared by every screen that reads it.
//
// Why this exists. Every view owned its own `useEffect(() => fetchX().then(setX))`, so the
// balance was re-fetched on Home, again on Wallet, again on Deposit, again on Withdraw,
// again on Invest — and each arrival was preceded by a skeleton. Nothing was wrong with any
// one of those reads; the problem was that leaving a page threw its answer away, so moving
// between two screens of the SAME account looked like loading a different account.
//
// Why not Next's fetch cache. `fetch(url, { next: { revalidate, tags } })` is a SERVER
// extension — it only applies to fetches Next itself issues while rendering. This cabinet
// reads nothing on the server: `/api/*` is a rewrite straight to the Rust BFF
// (next.config.ts), there are no route handlers, and every read is a browser call from a
// client component carrying the user's session cookie. So the cache has to live in the
// browser, at the one transport chokepoint that does exist (`shared/lib/api-client.ts`,
// wrapped by the entity clients). The vocabulary here is deliberately Next's — `revalidate`
// in seconds, `tags`, and a `revalidateTag()` that mutations call — because the semantics
// are the same and there is no reason to invent a second dialect for them.
//
// Why not a store library. The repo already answers this: `entities/user/model/
// profile-store.ts`, `entities/notification/model/notification-store.ts`,
// `shared/lib/use-session.ts` and `shared/lib/use-platform.ts` are four hand-rolled copies
// of this same pattern, each with slightly different staleness rules. This is that pattern
// written once, with a policy per resource instead of per copy — and no new dependency
// (see AGENTS.md § Hard rules).
//
// The behaviour that removes the skeleton: `useResource` serves whatever is already in the
// cache on the FIRST render, synchronously. A revalidation, when the value has aged past
// its `revalidate` window, runs behind the figure already on screen and swaps it in place —
// so `isLoading` is true only when there is genuinely nothing to show yet, which is the
// only state that has ever earned a skeleton. A failed revalidation keeps the stale value
// and reports the error beside it: a money surface must not blank a balance because one
// poll timed out.

import { useCallback, useSyncExternalStore } from "react";

/** Seconds a value stays fresh when a resource doesn't name its own window. */
const DEFAULT_REVALIDATE_S = 15;

/** sessionStorage namespace for `persist` resources — see `dropPersisted` for the scope rules. */
const PERSIST_PREFIX = "ev.cabinet.resource:";

export interface ResourceSnapshot<T> {
  /** The last known value, from this page or any earlier one. */
  data: T | undefined;
  /** The most recent attempt failed. Stale `data`, if any, is still present beside it. */
  error: Error | null;
  /** Nothing to show and nothing has failed — the only state that earns a skeleton. */
  isLoading: boolean;
  /** A refresh is running behind a value that is already on screen. */
  isValidating: boolean;
  /** Force a read now, ignoring freshness (a retry button, a pull-to-refresh). */
  refresh: () => Promise<void>;
}

export interface ResourceConfig<T, A extends unknown[]> {
  /** Stable identity — the cache key prefix, and what `clearResources` reports on. */
  name: string;
  /** The underlying read. One per endpoint; the entity clients supply these. */
  fetch: (...args: A) => Promise<T>;
  /** Key material for a parameterised read (`fundNav(service)`). Omit for a read with no arguments. */
  key?: (...args: A) => string;
  /** Seconds the cached value is served without a background refresh. */
  revalidate?: number;
  /** Invalidation groups a mutation can name through `revalidateTag`. */
  tags?: readonly string[];
  /**
   * Mirror into sessionStorage so a hard reload also paints from cache.
   *
   * NON-PERSONAL data only. The catalog of open funds and their NAVs qualify; a balance,
   * a position, a profile or an operation does not — those stay in memory, which dies with
   * the page. (`features/account-chip/model/identity-cache.ts` makes the same call for the
   * two identity fields it needs.)
   */
  persist?: boolean;
  /**
   * Whether these arguments describe a read worth making. A resource keyed on a value the
   * screen hasn't chosen yet (no fund selected) reports "not loading, no data" instead of
   * issuing a request the BFF would reject.
   */
  enabled?: (...args: A) => boolean;
}

/** `useResource`'s private door into a resource's entry map, kept off the public surface. */
const INTERNALS = Symbol("resource.internals");

export interface Resource<T, A extends unknown[]> {
  readonly [INTERNALS]: {
    ensure: (key: string, args: A) => Entry<T>;
    enabled: (...args: A) => boolean;
  };
  readonly name: string;
  /** The cache key these arguments resolve to — the unit `invalidate` and `publish` act on. */
  keyOf: (...args: A) => string;
  /** Cached value if fresh, otherwise a read. Concurrent callers share one request. */
  read: (...args: A) => Promise<T>;
  /** Whatever is cached right now, without subscribing or fetching. */
  peek: (...args: A) => T | undefined;
  /** Warm the cache ahead of a navigation. Fire-and-forget; failures are ignored. */
  prefetch: (...args: A) => void;
  /** Write a value straight in — for a mutation whose response IS the new state. */
  publish: (value: T, ...args: A) => void;
  /** Mark one key stale; refresh it now if a screen is showing it. */
  invalidate: (...args: A) => void;
  /** The same, for every key this resource has cached (all NAVs, all pages). */
  invalidateAll: () => void;
}

interface Entry<T> {
  readonly key: string;
  readonly load: () => Promise<T>;
  readonly freshMs: number;
  readonly tags: readonly string[];
  readonly persistKey: string | null;
  data: T | undefined;
  error: Error | null;
  /** 0 until the first successful read — the "never loaded" marker `isStale` reads. */
  fetchedAt: number;
  inflight: Promise<void> | null;
  readonly listeners: Set<() => void>;
  /** Referentially stable between changes: `useSyncExternalStore` compares it by identity. */
  snapshot: ResourceSnapshot<T>;
  refresh: () => Promise<void>;
}

const REGISTRY = new Map<string, Entry<unknown>>();

const NOOP_REFRESH = () => Promise.resolve();

// Server renders share module scope across every request, so nothing is cached there and no
// entry is created: a client component rendered on the server reports "loading" and picks
// up the real snapshot when it hydrates. `disabled` is the same shape minus the wait.
const EMPTY_SNAPSHOT = Object.freeze({ data: undefined, error: null, isLoading: true, isValidating: false, refresh: NOOP_REFRESH });
const DISABLED_SNAPSHOT = Object.freeze({ data: undefined, error: null, isLoading: false, isValidating: false, refresh: NOOP_REFRESH });

function emptySnapshot<T>(): ResourceSnapshot<T> {
  return EMPTY_SNAPSHOT as ResourceSnapshot<T>;
}

function disabledSnapshot<T>(): ResourceSnapshot<T> {
  return DISABLED_SNAPSHOT as ResourceSnapshot<T>;
}

function toError(cause: unknown): Error {
  return cause instanceof Error ? cause : new Error(String(cause));
}

function publishSnapshot<T>(entry: Entry<T>): void {
  entry.snapshot = {
    data: entry.data,
    error: entry.error,
    isLoading: entry.data === undefined && entry.error === null,
    isValidating: entry.inflight !== null,
    refresh: entry.refresh,
  };
  for (const listener of entry.listeners) listener();
}

function isStale(entry: Entry<unknown>): boolean {
  return entry.fetchedAt === 0 || Date.now() - entry.fetchedAt >= entry.freshMs;
}

function revalidate<T>(entry: Entry<T>): Promise<void> {
  if (entry.inflight) return entry.inflight;
  entry.inflight = entry
    .load()
    .then((value) => {
      entry.data = value;
      entry.error = null;
      entry.fetchedAt = Date.now();
      writePersisted(entry);
    })
    .catch((cause: unknown) => {
      // Deliberately non-destructive: the stale value stays, so a timed-out poll never
      // blanks a figure the user is reading.
      entry.error = toError(cause);
    })
    .finally(() => {
      entry.inflight = null;
      publishSnapshot(entry);
    });
  publishSnapshot(entry);
  return entry.inflight;
}

function writePersisted<T>(entry: Entry<T>): void {
  if (!entry.persistKey) return;
  try {
    sessionStorage.setItem(entry.persistKey, JSON.stringify({ v: entry.data, t: entry.fetchedAt }));
  } catch {
    // Private mode, quota, disabled storage — the in-memory cache is unaffected.
  }
}

function readPersisted<T>(entry: Entry<T>): void {
  if (!entry.persistKey) return;
  try {
    const raw = sessionStorage.getItem(entry.persistKey);
    if (!raw) return;
    const stored = JSON.parse(raw) as { v: T; t: number };
    entry.data = stored.v;
    entry.fetchedAt = stored.t;
    publishSnapshot(entry);
  } catch {
    // Unreadable or written by an older shape — fall through to a live read.
  }
}

function dropPersisted(entry: Entry<unknown>): void {
  if (!entry.persistKey) return;
  try {
    sessionStorage.removeItem(entry.persistKey);
  } catch {
    /* nothing to drop */
  }
}

function markStale(entry: Entry<unknown>): void {
  entry.fetchedAt = 0;
  dropPersisted(entry);
  // On screen right now: refresh in place, so a mutation's effect lands without the user
  // navigating. Not mounted: the next screen that reads it will find it stale and refresh.
  if (entry.listeners.size > 0) void revalidate(entry);
}

/**
 * Mark every cached read carrying any of these tags stale — the counterpart to Next's
 * `revalidateTag`, and how a mutation says what it changed. A withdrawal moves the balance,
 * the withdrawal list and the activity timeline; naming those three tags is the whole
 * contract, and no call site has to know which screens are currently mounted.
 */
export function revalidateTag(...tags: readonly string[]): void {
  for (const entry of REGISTRY.values()) {
    if (tags.some((tag) => entry.tags.includes(tag))) markStale(entry);
  }
}

/**
 * Drop every cached value. For sign-out: the next account to use this tab must never be
 * shown a figure belonging to the previous one. Entries are reset in place rather than
 * removed, so a component still mounted mid-teardown re-renders empty instead of holding a
 * reference to an orphaned entry.
 */
export function clearResources(): void {
  for (const entry of REGISTRY.values()) {
    dropPersisted(entry);
    entry.data = undefined;
    entry.error = null;
    entry.fetchedAt = 0;
    publishSnapshot(entry);
  }
}

// Coming back to a tab that has been in the background is exactly when the numbers on it
// are most likely to be old — and a backgrounded tab's timers are throttled, so nothing
// else would have caught it. Only mounted, stale entries are refreshed, so returning to a
// tab costs at most one read per figure actually on screen. Same reasoning as the unread
// badge's visibility check in `entities/notification/model/notification-store.ts`.
let watchingFocus = false;

function watchFocus(): void {
  if (watchingFocus || typeof document === "undefined") return;
  watchingFocus = true;
  const sweep = () => {
    if (document.visibilityState !== "visible") return;
    for (const entry of REGISTRY.values()) {
      if (entry.listeners.size > 0 && isStale(entry)) void revalidate(entry);
    }
  };
  document.addEventListener("visibilitychange", sweep);
  window.addEventListener("online", sweep);
}

export function defineResource<T, A extends unknown[] = []>(config: ResourceConfig<T, A>): Resource<T, A> {
  const freshMs = (config.revalidate ?? DEFAULT_REVALIDATE_S) * 1000;
  const tags = config.tags ?? [];

  const keyOf = (...args: A): string => `${config.name}(${config.key ? config.key(...args) : ""})`;

  function ensure(key: string, args: A): Entry<T> {
    const existing = REGISTRY.get(key) as Entry<T> | undefined;
    if (existing) return existing;
    const entry: Entry<T> = {
      key,
      load: () => config.fetch(...args),
      freshMs,
      tags,
      persistKey: config.persist ? `${PERSIST_PREFIX}${key}` : null,
      data: undefined,
      error: null,
      fetchedAt: 0,
      inflight: null,
      listeners: new Set(),
      snapshot: emptySnapshot<T>(),
      refresh: NOOP_REFRESH,
    };
    entry.refresh = () => {
      entry.fetchedAt = 0;
      return revalidate(entry);
    };
    REGISTRY.set(key, entry as Entry<unknown>);
    readPersisted(entry);
    publishSnapshot(entry);
    watchFocus();
    return entry;
  }

  const enabled = (...args: A) => config.enabled?.(...args) ?? true;

  return {
    [INTERNALS]: { ensure, enabled },
    name: config.name,
    keyOf,
    read(...args: A): Promise<T> {
      const entry = ensure(keyOf(...args), args);
      if (!isStale(entry) && entry.data !== undefined) return Promise.resolve(entry.data);
      return revalidate(entry).then(() => {
        if (entry.data !== undefined) return entry.data;
        throw entry.error ?? new Error(`${config.name} unavailable`);
      });
    },
    peek(...args: A): T | undefined {
      return REGISTRY.get(keyOf(...args))?.data as T | undefined;
    },
    prefetch(...args: A): void {
      if (typeof window === "undefined" || !enabled(...args)) return;
      const entry = ensure(keyOf(...args), args);
      if (isStale(entry)) void revalidate(entry);
    },
    publish(value: T, ...args: A): void {
      const entry = ensure(keyOf(...args), args);
      entry.data = value;
      entry.error = null;
      entry.fetchedAt = Date.now();
      writePersisted(entry);
      publishSnapshot(entry);
    },
    invalidate(...args: A): void {
      const entry = REGISTRY.get(keyOf(...args));
      if (entry) markStale(entry);
    },
    invalidateAll(): void {
      const prefix = `${config.name}(`;
      for (const entry of REGISTRY.values()) {
        if (entry.key.startsWith(prefix)) markStale(entry);
      }
    },
  };
}

/**
 * Read a resource, re-rendering when its value changes.
 *
 * Returns cached data on the first render — that is the point. A screen revisited inside
 * the same session paints its real figures immediately and refreshes them behind itself;
 * only a genuinely cold read reports `isLoading`.
 */
export function useResource<T, A extends unknown[]>(resource: Resource<T, A>, ...args: A): ResourceSnapshot<T> {
  const internals = resource[INTERNALS];
  const key = resource.keyOf(...args);
  const active = internals.enabled(...args);
  // Creating the entry during render is a lookup-or-insert into a module map: idempotent,
  // observable by nobody, and it never starts a request — subscribing does. Doing it here
  // is what gives `getSnapshot` a stable entry to read on the very first render, which is
  // the difference between painting cached data and painting a skeleton. Skipped on the
  // server, where module scope is shared between users.
  const entry = typeof window === "undefined" || !active ? null : internals.ensure(key, args);

  const subscribe = useCallback(
    (onChange: () => void) => {
      if (!entry) return () => undefined;
      entry.listeners.add(onChange);
      if (isStale(entry)) void revalidate(entry);
      return () => {
        entry.listeners.delete(onChange);
      };
    },
    [entry],
  );

  const fallback = active ? emptySnapshot<T> : disabledSnapshot<T>;
  return useSyncExternalStore(subscribe, () => entry?.snapshot ?? fallback(), fallback);
}

/** Test seam: forget every cached value AND every entry, so each case starts cold. */
export function resetResourcesForTests(): void {
  REGISTRY.clear();
}

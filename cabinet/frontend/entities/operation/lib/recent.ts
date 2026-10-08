// Plain, not under `"use client"`: the home route reads the same page on the server, and a
// constant imported from a client module arrives there as a client reference, not a number.

/** How many operations Home's preview card asks the hub for — the cache key its warm-up uses. */
export const RECENT_OPS = 6;

// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { rangeFrom, toNavSeries } from "./nav-series.ts";

const DAY_S = 86_400;
// 2026-09-17T10:20:30Z — mid-day, so the flooring below has something to floor.
const NOW_MS = Date.UTC(2026, 8, 17, 10, 20, 30);

test("a range opens a fixed number of days back, floored to the start of a UTC day", () => {
  assert.equal(rangeFrom("1m", NOW_MS), Date.UTC(2026, 7, 18) / 1000);
  assert.equal(rangeFrom("6m", NOW_MS), Date.UTC(2026, 8, 17) / 1000 - 182 * DAY_S);
  assert.equal(rangeFrom("1y", NOW_MS), Date.UTC(2025, 8, 17) / 1000);
  assert.equal(rangeFrom("1m", NOW_MS)! % DAY_S, 0);
});

test("the same range on the same day is the same bound, so the cache key holds", () => {
  assert.equal(rangeFrom("1m", NOW_MS), rangeFrom("1m", Date.UTC(2026, 8, 17, 23, 59, 59)));
  assert.notEqual(rangeFrom("1m", NOW_MS), rangeFrom("1m", Date.UTC(2026, 8, 18, 0, 0, 0)));
});

test("all-time has no lower bound", () => {
  assert.equal(rangeFrom("all", NOW_MS), undefined);
});

test("marks become a percent return rebased on the first mark; participation stays in USDT", () => {
  const series = toNavSeries({
    allocation: "arb",
    marks: [
      { nav: "1.0000", aum: "100", posted_at: "1000" },
      { nav: "1.0500", aum: "105", posted_at: "2000" },
      { nav: "0.9800", aum: "98", posted_at: "3000" },
    ],
    participation: [
      { at: "1000", value: "50.00" },
      { at: "3000", value: "49.00" },
    ],
    truncated: false,
  });
  assert.equal(series.baseNav, 1);
  assert.deepEqual(
    series.performance.map((p) => [p.time, Math.round(p.value * 100) / 100]),
    [
      [1000, 0],
      [2000, 5],
      [3000, -2],
    ],
  );
  assert.deepEqual(series.participation, [
    { time: 1000, value: 50 },
    { time: 3000, value: 49 },
  ]);
});

test("stamps arrive as numbers or strings; both are read, and unsorted input comes out ascending", () => {
  const series = toNavSeries({
    marks: [
      { nav: "2", posted_at: 3000 },
      { nav: "1", posted_at: "1000" },
    ],
  });
  assert.deepEqual(
    series.performance.map((p) => p.time),
    [1000, 3000],
  );
  assert.equal(series.baseNav, 1);
});

test("two marks in the same second keep the one posted last", () => {
  const series = toNavSeries({ marks: [{ nav: "1", posted_at: "1000" }, { nav: "1", posted_at: "2000" }, { nav: "3", posted_at: "2000" }] });
  assert.deepEqual(series.performance, [
    { time: 1000, value: 0 },
    { time: 2000, value: 200 },
  ]);
});

test("a mark without a stamp, with a zero stamp, or with an unreadable NAV is skipped rather than plotted at 1970", () => {
  const series = toNavSeries({
    marks: [{ nav: "1" }, { nav: "1", posted_at: "0" }, { nav: "abc", posted_at: "1000" }, { nav: "0", posted_at: "1500" }, { nav: "1.5", posted_at: "2000" }],
    participation: [{ at: "0", value: "1" }, { at: "2000", value: "x" }],
  });
  assert.deepEqual(series.performance, [{ time: 2000, value: 0 }]);
  assert.deepEqual(series.participation, []);
});

test("a seed-only fund, a non-holder and an unread history are all empty series, not errors", () => {
  assert.deepEqual(toNavSeries({ allocation: "arb", marks: [], participation: [], truncated: false }), { performance: [], participation: [], baseNav: null });
  assert.deepEqual(toNavSeries(undefined), { performance: [], participation: [], baseNav: null });
});

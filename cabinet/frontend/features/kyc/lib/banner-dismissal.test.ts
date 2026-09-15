// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test, { beforeEach } from "node:test";

import { DISMISS_MS, dismissBanner, isBannerDismissed } from "./banner-dismissal.ts";

/** A `localStorage` that can also be made to throw, the way a private window does. */
function stub({ throws = false } = {}) {
  const map = new Map<string, string>();
  const guard = () => {
    if (throws) throw new DOMException("The operation is insecure.", "SecurityError");
  };
  (globalThis as { window?: unknown }).window = {
    localStorage: {
      getItem: (k: string) => {
        guard();
        return map.get(k) ?? null;
      },
      setItem: (k: string, v: string) => {
        guard();
        map.set(k, v);
      },
      removeItem: (k: string) => {
        guard();
        map.delete(k);
      },
    },
  };
  return map;
}

const NOW = 1_789_459_200_000;

beforeEach(() => {
  stub();
});

test("a fresh browser has not dismissed the banner", () => {
  assert.equal(isBannerDismissed(NOW), false);
});

test("a dismissal holds for seven days and then lapses", () => {
  dismissBanner(NOW);
  assert.equal(isBannerDismissed(NOW), true);
  assert.equal(isBannerDismissed(NOW + DISMISS_MS - 1), true);
  assert.equal(isBannerDismissed(NOW + DISMISS_MS), false, "a banner put away forever stops onboarding anyone");
});

test("a lapsed dismissal is cleared rather than re-read on every mount", () => {
  const store = stub();
  dismissBanner(NOW);
  isBannerDismissed(NOW + DISMISS_MS);
  assert.equal(store.size, 0);
});

test("a nonsense value counts as no dismissal, not as forever", () => {
  const store = stub();
  dismissBanner(NOW);
  const key = [...store.keys()][0]!;
  store.set(key, "soon");
  assert.equal(isBannerDismissed(NOW), false);
});

test("storage that throws is not a blank page", () => {
  stub({ throws: true });
  assert.equal(isBannerDismissed(NOW), false);
  assert.doesNotThrow(() => dismissBanner(NOW));
});

// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test, { beforeEach } from "node:test";

import { acknowledge, completionView, markOpen, readStage } from "./checklist-memory.ts";

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

beforeEach(() => {
  stub();
});

test("a fresh browser remembers nothing", () => {
  assert.equal(readStage(), null);
});

test("seeing an open step, then putting the completion away", () => {
  markOpen();
  assert.equal(readStage(), "open");
  acknowledge();
  assert.equal(readStage(), "acknowledged");
});

test("an acknowledged checklist does not reopen", () => {
  // A verified reader whose balance went to zero and back would otherwise be congratulated
  // a second time.
  acknowledge();
  markOpen();
  assert.equal(readStage(), "acknowledged");
});

test("a finished path shows the completion once, then the line, and nothing to a stranger", () => {
  assert.equal(completionView("open"), "all-set");
  assert.equal(completionView("acknowledged"), "line");
  // A veteran in a fresh browser never saw the checklist: no line for a path never walked.
  assert.equal(completionView(null), null);
});

test("a nonsense value counts as nothing remembered", () => {
  const store = stub();
  markOpen();
  const key = [...store.keys()][0]!;
  store.set(key, "done");
  assert.equal(readStage(), null);
});

test("storage that throws is not a blank page", () => {
  stub({ throws: true });
  assert.equal(readStage(), null);
  assert.doesNotThrow(() => markOpen());
  assert.doesNotThrow(() => acknowledge());
});

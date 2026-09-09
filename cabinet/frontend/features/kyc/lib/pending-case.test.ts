import assert from "node:assert/strict";
import { test } from "node:test";

import { isVerificationPending, markVerificationPending } from "./pending-case.ts";

// Mirrors the module's private key — there is no public seam to inject an aged
// timestamp other than writing the same shape it would have written itself.
const PENDING_KEY = "ev.cabinet.kyc-pending";
const A_DAY_MS = 24 * 60 * 60 * 1000;

class MemoryStorage {
  #store = new Map<string, string>();
  getItem(key: string) {
    return this.#store.has(key) ? this.#store.get(key)! : null;
  }
  setItem(key: string, value: string) {
    this.#store.set(key, value);
  }
  removeItem(key: string) {
    this.#store.delete(key);
  }
}

function withStorage(fn: (storage: MemoryStorage) => void) {
  const storage = new MemoryStorage();
  (globalThis as { sessionStorage?: unknown }).sessionStorage = storage;
  try {
    fn(storage);
  } finally {
    delete (globalThis as { sessionStorage?: unknown }).sessionStorage;
  }
}

test("no case has been started: nothing reads as pending", () => {
  withStorage(() => {
    assert.equal(isVerificationPending("user@example.com"), false);
  });
});

test("a case just started reads back as pending for the same email", () => {
  withStorage(() => {
    markVerificationPending("user@example.com");
    assert.equal(isVerificationPending("user@example.com"), true);
  });
});

test("a different account on the same browser never inherits the flag", () => {
  withStorage(() => {
    markVerificationPending("user@example.com");
    assert.equal(isVerificationPending("someone-else@example.com"), false);
  });
});

test("a flag older than the TTL stops counting as pending and is cleared", () => {
  withStorage((storage) => {
    storage.setItem(PENDING_KEY, JSON.stringify({ email: "user@example.com", startedAt: Date.now() - (A_DAY_MS + 1000) }));
    assert.equal(isVerificationPending("user@example.com"), false);
    assert.equal(storage.getItem(PENDING_KEY), null);
  });
});

test("a fresh flag just under the TTL still counts as pending", () => {
  withStorage((storage) => {
    storage.setItem(PENDING_KEY, JSON.stringify({ email: "user@example.com", startedAt: Date.now() - (A_DAY_MS - 1000) }));
    assert.equal(isVerificationPending("user@example.com"), true);
  });
});

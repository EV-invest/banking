// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The schema's messages are keys, resolved to words only in `validateProfileForm`. A message
// that missed its table entry used to reach the screen as `err.…` verbatim; the union now
// makes that a type error, and these pin the runtime half: every rule's message resolves,
// and to the entry for its own key.
import assert from "node:assert/strict";
import test from "node:test";

import type { Translate } from "@evinvest/i18n";

import { PROFILE_ISSUES, profileEditableSchema, profileIssueWords, validateProfileForm } from "./profile-schema.ts";

// Answers with the key it was asked for, so a result proves which entry was resolved.
const t: Translate = (key) => `T:${key}`;

test("every issue the rules can raise has an entry that asks for its own key", () => {
  const words: Readonly<Record<string, string>> = profileIssueWords(t);
  for (const issue of PROFILE_ISSUES) assert.equal(words[issue], `T:${issue}`, issue);
  assert.deepEqual(Object.keys(words).sort(), [...PROFILE_ISSUES].sort());
});

// The form always carries every field; an empty string is "cleared", which every rule allows.
const BLANK: Record<string, string> = Object.fromEntries(Object.keys(profileEditableSchema.shape).map((field) => [field, ""]));

const CASES: [field: string, value: string, issue: (typeof PROFILE_ISSUES)[number]][] = [
  ["legal_name", "a".repeat(257), "err.field.maxLength"],
  ["legal_name", "Ab1", "err.field.nameChars"],
  ["legal_name", "A.", "err.field.minLetters"],
  ["phone", "+" + "1".repeat(32), "err.phone.maxLength"],
  ["phone", "abc", "err.phone.invalid"],
  ["date_of_birth", "1990/01/01", "err.dob.format"],
  ["date_of_birth", "1800-01-01", "err.dob.range"],
  ["residential_address", "a".repeat(257), "err.address.maxLength"],
  ["residential_address", "a\u0001b", "err.address.controlChars"],
  ["language", "a".repeat(17), "err.language.maxLength"],
  ["language", "e", "err.language.format"],
  ["base_currency", "US1", "err.currency.format"],
  ["timezone", "A".repeat(65), "err.timezone.maxLength"],
  ["timezone", "Mars/Base", "err.timezone.format"],
];

test("each rule's message on the form resolves to its own words, never a raw key", () => {
  for (const [field, value, issue] of CASES) {
    const errors = validateProfileForm({ ...BLANK, [field]: value }, t);
    assert.equal(errors[field], `T:${issue}`, `${field}=${JSON.stringify(value.slice(0, 20))}`);
  }
});

test("a valid form has no errors", () => {
  assert.deepEqual(validateProfileForm({ ...BLANK, legal_name: "Ada Lovelace", timezone: "UTC" }, t), {});
});

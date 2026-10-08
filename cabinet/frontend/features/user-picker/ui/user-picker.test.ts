// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The investor picker holds the whole directory and leaves the matching to the kit's
// `Command`, on email and user id together. These pin which rows a typed search keeps
// (banking#471: rows found by email used to vanish). Rendered with `react-dom/server` and
// the picker already open (see `__tests__/open-picker-kit.ts`); with no effects run there,
// `CommandEmpty` cannot tell results from none, so only a search with no rows checks it.
// Keyboard selection needs a browser and is covered by the stand scenario, not here.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { I18nProvider } from "@evinvest/i18n/react";
import { createElement, type ReactNode } from "react";
import { renderToString } from "react-dom/server";

import { installModuleHooks } from "../../../shared/__tests__/react-hook-harness.ts";
import { answered, directoryIs, investor, loading } from "./__tests__/fake-user-directory.ts";
import { typeIntoPicker } from "./__tests__/open-picker-kit.ts";

const directory = new URL("./__tests__/fake-user-directory.ts", import.meta.url).href;
installModuleHooks({
  "@evinvest/uikit": new URL("./__tests__/open-picker-kit.ts", import.meta.url).href,
  "@/shared/lib/resource": directory,
  "@/entities/admin/model/admin-resource": directory,
  "@/shared/ui/resource-error": directory,
});

const { UserPicker } = await import("./user-picker.tsx");

const ANNA = investor("0198a3c1-7c2e-7aa1-9f00-000000000001", "anna.investor@example.com");
const BORIS = investor("0198a3c1-7c2e-7aa1-9f00-000000000002", "boris.owner@example.com");

const messages = JSON.parse(readFileSync(new URL("../../../messages/en/common.json", import.meta.url), "utf8"));
const inEnglish = (children: ReactNode) =>
  // eslint-disable-next-line react/no-children-prop -- no JSX in a .ts test, and the provider's props type `children` as required
  createElement(I18nProvider, { locale: "en", messages, children });

const renderPicker = () => renderToString(inEnglish(createElement(UserPicker, { value: null, onPick: () => undefined })));

/** The visible label of every option row, in order. */
const optionLabels = (html: string) => [...html.matchAll(/role="option"[^>]*>.*?<span[^>]*>([^<]*)<\/span><\/div>/g)].map((m) => m[1]);

test("part of an email keeps that investor alone", () => {
  typeIntoPicker("anna.inv");
  directoryIs(answered([ANNA, BORIS]));

  assert.deepEqual(optionLabels(renderPicker()), ["anna.investor@example.com"]);
});

test("part of a user id keeps everyone whose id has it", () => {
  typeIntoPicker("9f00");
  directoryIs(answered([ANNA, BORIS]));

  assert.deepEqual(optionLabels(renderPicker()), ["anna.investor@example.com", "boris.owner@example.com"]);
});

test("letters of an email in order, not adjacent, still find it", () => {
  typeIntoPicker("bowner");
  directoryIs(answered([ANNA, BORIS]));

  assert.deepEqual(optionLabels(renderPicker()), ["boris.owner@example.com"]);
});

test("a search that matches nobody says no investors match", () => {
  typeIntoPicker("zzz-nobody");
  directoryIs(answered([ANNA, BORIS]));

  const html = renderPicker();

  assert.deepEqual(optionLabels(html), []);
  assert.match(html, /No investors match/);
});

test("while the directory is loading neither rows nor the no-match message show", () => {
  typeIntoPicker("anna.inv");
  directoryIs(loading());

  const html = renderPicker();

  assert.deepEqual(optionLabels(html), []);
  assert.doesNotMatch(html, /No investors match/);
});

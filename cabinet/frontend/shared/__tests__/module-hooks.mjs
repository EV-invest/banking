// Node module hooks for the React hook tests (registered by `react-hook-harness.ts`).
//
// Three redirects, nothing else:
// - a specifier the test file named in `stubs` resolves to the URL it gave — for a module
//   whose real body Node cannot load (type stripping refuses parameter properties, which
//   `shared/lib/api-client.ts` uses) and which the module under test only touches lightly;
// - `@/x` resolves the way tsconfig `paths` does, to `<frontend>/x.ts`, so a module under
//   test can be loaded as written instead of only through relative imports;
// - `lightweight-charts` resolves to `fake-lightweight-charts.mjs`, under a fresh query on
//   EVERY import, so each `import()` the hook makes is a download of its own and a test can
//   see whether the HOOK caches a failure. (An errored module record would otherwise replay
//   its error forever.) This models the hook's contract, not the bundler: Next 16's
//   Turbopack runtime keeps a failed chunk's rejected promise for the life of the page, so
//   in the built app a second `import()` after a failure does not reach the network.
//   The importer's `?case=` rides along, so a download a finished test left in flight
//   reports to that test's controller, not to whichever test is running when it lands.

import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

const ROOT = new URL("../../", import.meta.url);
const FAKE_ENGINE = new URL("./fake-lightweight-charts.mjs", import.meta.url);
let engineImports = 0;
/** @type {Record<string, string>} */
let stubs = {};

export function initialize(data) {
  stubs = data?.stubs ?? {};
}

export async function resolve(specifier, context, next) {
  const stub = stubs[specifier];
  if (stub) return { url: stub, shortCircuit: true };
  if (specifier.startsWith("@/")) {
    for (const suffix of [".ts", "/index.ts"]) {
      const url = new URL(specifier.slice(2) + suffix, ROOT);
      if (existsSync(fileURLToPath(url))) return { url: url.href, shortCircuit: true };
    }
  }
  if (specifier === "lightweight-charts") {
    engineImports += 1;
    const forCase = context.parentURL ? (new URL(context.parentURL).searchParams.get("case") ?? "") : "";
    return { url: `${FAKE_ENGINE.href}?import=${engineImports}&case=${encodeURIComponent(forCase)}`, format: "module", shortCircuit: true };
  }
  return next(specifier, context);
}

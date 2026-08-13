// CI gate for the translation policy (rules 1.1 / 1.2 — @evinvest/i18n/policy).
//
// The runtime already degrades safely: a drifted entry falls back to canonical
// English and the page renders. That safety is exactly why this exists — a silent
// fallback is indistinguishable from a surface that was never translated, so
// without a noisy second channel a locale can rot to zero coverage unnoticed.
//
// Fails only on *drift*, never on untranslated keys: a locale is filled in over
// time, and blocking CI on unfinished translation work only gets the check
// disabled.
import { auditCatalogues } from "@evinvest/i18n/policy";

import { catalogueReport } from "../shared/config/i18n";

const resolved = catalogueReport();
const { report } = auditCatalogues(resolved, 0);
console.log(report);

const drifted = resolved.flatMap((c) =>
  c.rejected.map((r) => `${c.locale}/${r.key}: ${r.reason} — ${r.detail}`),
);

if (drifted.length > 0) {
  console.error(`\n${drifted.length} entr${drifted.length === 1 ? "y" : "ies"} rejected by policy:`);
  for (const line of drifted) console.error(`  ${line}`);
  console.error(
    "\nEnglish is being served for these. Retranslate and update the `en` field," +
      " or revert the English change.",
  );
  process.exit(1);
}

console.log("\ni18n: no drift — every translation matches its English source");

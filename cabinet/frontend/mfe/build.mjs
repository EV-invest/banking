// Bundles the account-chip element remote into a single self-registering ESM file served
// from public/mfe/. React + the chip are bundled in (the host never imports this module
// graph — it only injects the <script>). The sibling CSS is compiled separately by the
// Tailwind CLI (see the `build:mfe` npm script) so this stays a pure JS build.
import { build } from "esbuild";

await build({
  entryPoints: ["mfe/account-chip/entry.tsx"],
  bundle: true,
  format: "esm",
  minify: true,
  target: ["es2022"],
  jsx: "automatic",
  outfile: "public/mfe/account-chip.js",
  // React reads process.env.NODE_ENV; there is no `process` in the browser, so inline it.
  define: {
    "process.env.NODE_ENV": '"production"',
    // The menu's Service-Arb entry; unset, it is left out.
    "process.env.NEXT_PUBLIC_SA_PANEL_URL":
      process.env.NEXT_PUBLIC_SA_PANEL_URL === undefined ? "undefined" : JSON.stringify(process.env.NEXT_PUBLIC_SA_PANEL_URL),
  },
  // Resolve the `@/*` path alias from the app tsconfig.
  tsconfig: "tsconfig.json",
  logLevel: "info",
});

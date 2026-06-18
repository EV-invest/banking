// eslint-config-next@16 ships native flat configs; spread them directly.
// (The old FlatCompat shim crashed under ESLint 9 — its legacy validator
// JSON.stringify'd eslint-plugin-react's self-referential config and threw on
// the circular structure before linting a single file.)
import next from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

const eslintConfig = [
  ...next,
  ...nextTs,
  {
    // shared/api/generated is an auto-generated artifact (npm run gen:api);
    // it's already prettier-formatted by codegen, so keep it out of lint churn.
    ignores: [".next/**", "node_modules/**", "dist/**", "shared/api/generated/**"],
  },
  {
    rules: {
      // The shadcn/ui kit and a couple of generic helpers lean on `any`;
      // surface it as a warning rather than failing the build.
      "@typescript-eslint/no-explicit-any": "warn",
      // Marketing copy legitimately contains quotes/apostrophes in JSX text.
      "react/no-unescaped-entities": "off",
      // A couple of client-only hooks set state in an effect to stay SSR-safe
      // (window/cookie reads can't run during render). Warn, don't fail.
      "react-hooks/set-state-in-effect": "warn",
    },
  },
];

export default eslintConfig;

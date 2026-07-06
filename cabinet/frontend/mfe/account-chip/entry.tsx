import { createRoot, type Root } from "react-dom/client";

import { AccountChip } from "@/features/account-chip";

// Self-registering ESM entry for the account-chip element remote. The conductor injects
// this bundle (`site_conductor mfe-registry` → `/cabinet/mfe/account-chip.js`) and mounts
// the <mfe-cabinet-account-chip> custom element in its shared header. React renders into
// the element's LIGHT DOM (not a shadow root) so the host's uikit tokens/fonts cascade in;
// the chip's own utilities ship in a sibling stylesheet pre-layered `@layer reamfe`, which
// the host keeps below its own styles. See the real_estate_allocation embed for the
// prior-art conventions (self-register on import, guard double-registration, derive asset
// URLs from the bundle's own location).

const TAG = "mfe-cabinet-account-chip";

// Sibling stylesheet next to this bundle. Built via string math (not `new URL(…,
// import.meta.url)`) so esbuild doesn't try to resolve/inline it at build time — the CSS
// is emitted separately by the Tailwind CLI, not part of the JS graph.
function ensureStylesheet() {
  if (document.querySelector(`link[data-mfe="${TAG}"]`)) return;
  const src = import.meta.url;
  const href = src.slice(0, src.lastIndexOf("/") + 1) + "account-chip.css";
  const link = document.createElement("link");
  link.rel = "stylesheet";
  link.href = href;
  link.dataset.mfe = TAG;
  document.head.appendChild(link);
}

if (!customElements.get(TAG)) {
  customElements.define(
    TAG,
    class extends HTMLElement {
      #root?: Root;
      connectedCallback() {
        ensureStylesheet();
        this.#root = createRoot(this);
        this.#root.render(<AccountChip />);
      }
      disconnectedCallback() {
        this.#root?.unmount();
        this.#root = undefined;
      }
    },
  );
}

import * as React from "react";
import { Slot } from "@radix-ui/react-slot";

import { cn } from "@/shared/lib/utils";

// ─────────────────────────────────────────────────────────────────────────────
// Document typography.
//
// `Text` and `Heading` (re-exported below from ./text) are the *card-scale*
// primitives: a heading pins a card's body size via context. They're for the
// chrome of the marketing sections.
//
// The set here — H1..H6, P — is the *document* scale: long-form, markdown-like
// prose (legal pages, research notes, terms). They carry the brand fonts/colors
// so a rendered .md reads as ours without per-element styling. Each is
// polymorphic via `asChild` (drop a custom node, keep the styling) and every
// class is overridable through `className`.
// ─────────────────────────────────────────────────────────────────────────────

type AsChild = { asChild?: boolean };

function make<Tag extends keyof React.JSX.IntrinsicElements>(
  tag: Tag,
  slot: string,
  base: string
) {
  function Component({
    className,
    asChild = false,
    ...props
  }: React.ComponentProps<Tag> & AsChild) {
    const Comp = asChild ? Slot : tag;
    return (
      // @ts-expect-error — polymorphic tag/props are sound at every call site.
      <Comp data-slot={slot} className={cn(base, className)} {...props} />
    );
  }
  Component.displayName = slot;
  return Component;
}

const H1 = make(
  "h1",
  "h1",
  "font-serif-display font-bold text-white text-4xl sm:text-5xl tracking-tight leading-tight mb-6"
);

const H2 = make(
  "h2",
  "h2",
  "font-serif-display font-bold text-white text-3xl sm:text-4xl tracking-tight leading-tight mt-12 mb-5"
);

const H3 = make(
  "h3",
  "h3",
  "font-serif-display text-white text-2xl sm:text-3xl leading-snug mt-10 mb-4"
);

const H4 = make(
  "h4",
  "h4",
  "font-serif-display text-white text-xl sm:text-2xl leading-snug mt-8 mb-3"
);

const H5 = make(
  "h5",
  "h5",
  "font-sans font-semibold text-white text-lg leading-snug mt-6 mb-2"
);

// h6 reads as an eyebrow/caption — the recurring uppercase mono label.
const H6 = make(
  "h6",
  "h6",
  "font-mono-tech text-main-accent-t1 text-xs uppercase tracking-[0.3em] mt-6 mb-2"
);

const P = make(
  "p",
  "p",
  "font-sans font-light leading-relaxed text-main-mist/70 text-sm sm:text-base [&:not(:first-child)]:mt-4"
);

export { H1, H2, H3, H4, H5, H6, P };
export { Heading, Text, textVariants } from "./text";

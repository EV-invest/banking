"use client";

import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/shared/lib/utils";

/**
 * A card/block has ONE scale, set by its <Heading>. The heading size and the
 * body (info) size are pinned together so they can never drift:
 *   main — h3 text-2xl, body text-sm   (prominent cards: Horizon, Calculator)
 *   alt  — h3 text-xl,  body text-xs   (compact cards: side asset, bento)
 * The <Heading> publishes its scale via context; every <Text> below inherits
 * the matching body size automatically. Size is still overridable via className.
 */
type Scale = "main" | "alt";

const HEADING_SIZE: Record<Scale, string> = {
  main: "text-2xl sm:text-3xl",
  alt: "text-xl sm:text-2xl",
};

const BODY_SIZE: Record<Scale, string> = {
  main: "text-sm",
  alt: "text-xs",
};

const ScaleContext = React.createContext<Scale>("main");

function Heading({
  className,
  scale = "main",
  asChild = false,
  ...props
}: React.ComponentProps<"h3"> & { scale?: Scale; asChild?: boolean }) {
  const Comp = asChild ? Slot : "h3";
  return (
    <ScaleContext.Provider value={scale}>
      <Comp
        data-slot="heading"
        className={cn(
          HEADING_SIZE[scale],
          "font-serif-display text-white mb-4",
          className
        )}
        {...props}
      />
    </ScaleContext.Provider>
  );
}

/**
 * Two text colors, named by role. The page text is Limestone Mist (--color-main-mist)
 * shown at two opacities:
 *   info      — primary reading copy. font-light leading-relaxed; size is pinned to
 *               the surrounding Heading scale (main=text-sm, alt=text-xs).
 *   secondary — quieter supporting copy: stat labels, captions, field labels, footer.
 *               COLOR ONLY — these share nothing but the opacity, so casing, size and
 *               font (e.g. font-mono-tech uppercase) stay on the caller; no scale.
 */
const textVariants = cva("", {
  variants: {
    variant: {
      info: "font-light leading-relaxed text-main-mist/70",
      secondary: "text-main-mist/40",
    },
  },
  defaultVariants: {
    variant: "info",
  },
});

function Text({
  className,
  variant = "info",
  asChild = false,
  ...props
}: React.ComponentProps<"p"> &
  VariantProps<typeof textVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot : "p";
  const scale = React.useContext(ScaleContext);
  // Only info inherits the scale's body size; secondary owns its own sizing.
  const size = variant === "secondary" ? undefined : BODY_SIZE[scale];

  return (
    <Comp
      data-slot="text"
      className={cn(textVariants({ variant }), size, className)}
      {...props}
    />
  );
}

export { Heading, Text, textVariants };

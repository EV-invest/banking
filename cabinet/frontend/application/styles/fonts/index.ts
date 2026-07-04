import localFont from "next/font/local";

// Self-hosted via next/font (no render-blocking <link> to Google Fonts).
// Each exposes a CSS variable consumed by globals.css / the Tailwind theme.
// Inter is the single neutral workhorse: it backs both the sans body copy and
// the "mono-tech" labels (tracked-out, uppercase) — replacing the former
// Space Grotesk / Space Mono pairing with one quieter, institutional grotesque.
// Self-hosted from the variable .ttf (not next/font/google) so the production
// image builds hermetically — no Google fetch in the nix sandbox.
export const fontInter = localFont({
  src: [
    {
      path: "./Inter-VariableFont_opsz,wght.ttf",
      style: "normal",
      weight: "100 900",
    },
    {
      path: "./Inter-Italic-VariableFont_opsz,wght.ttf",
      style: "italic",
      weight: "100 900",
    },
  ],
  display: "swap",
  variable: "--font-inter",
});

// Playfair: the redesigned variable family (opsz/wdth/wght), self-hosted from
// the .ttf files in this folder rather than next/font/google — one upstream,
// full axis range, no Google fetch. One src entry per style; the variable wght
// axis spans 300–900 so a single file covers every weight we use.
export const fontPlayfair = localFont({
  src: [
    {
      path: "./Playfair-VariableFont_opsz,wdth,wght.ttf",
      style: "normal",
      weight: "300 900",
    },
    {
      path: "./Playfair-Italic-VariableFont_opsz,wdth,wght.ttf",
      style: "italic",
      weight: "300 900",
    },
  ],
  display: "swap",
  variable: "--font-playfair",
});


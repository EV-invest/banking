import localFont from "next/font/local";

// Self-hosted via next/font (no render-blocking <link> to Google Fonts).
// Exposes a CSS variable consumed by globals.css / the Tailwind theme.
// Inter is the only face the cabinet ships: it backs the sans body copy, the
// "mono-tech" labels (tracked-out, uppercase) and the headings that once used
// the Playfair display serif — one quieter, institutional grotesque throughout.
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

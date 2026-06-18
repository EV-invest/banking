import { expect, test } from "@playwright/test";

// One visual-regression baseline per section of the site. `name` names the
// screenshot file (__screenshots__/<name>.png); `selector` locates the section.
// The anchored content sections (hero, portfolio, calculator, research, team)
// are reachable by their nav hash, e.g. http://localhost:3000/#team — those use
// an id selector. The page chrome (header/footer) has no anchor, so it's keyed
// by tag.
//
// Adding a section to the site = adding one line here. Nothing else changes.
const SECTIONS = [
  { name: "header", selector: "header" },
  { name: "hero", selector: "#hero" },
  { name: "portfolio", selector: "#portfolio" },
  { name: "calculator", selector: "#calculator" },
  { name: "research", selector: "#research" },
  { name: "team", selector: "#team" },
  { name: "footer", selector: "footer" },
] as const;

// The hero scales its background with window.scrollY; pinning the scroll
// position makes its zoom (and the header's blur-on-scroll state) deterministic.
// Top-of-page for the chrome sections, the element itself otherwise.
const PIN_TO_TOP = new Set<string>(["header", "hero"]);

for (const { name, selector } of SECTIONS) {
  test(`- mismatch on: ${name}`, async ({ page }) => {
    await page.goto("/");

    // Web fonts shift glyph metrics; wait until they're applied.
    await page.evaluate(() => document.fonts.ready);
    // CloudFront background images load over the network, not via <img> decode.
    await page.waitForLoadState("networkidle");

    const section = page.locator(selector);
    await expect(section).toBeVisible();

    if (PIN_TO_TOP.has(name)) {
      await page.evaluate(() => window.scrollTo(0, 0));
    } else {
      await section.scrollIntoViewIfNeeded();
    }
    // Let the scroll-driven transform settle to its resting frame.
    await page.waitForTimeout(150);

    await expect(section).toHaveScreenshot(`${name}.png`);
  });
}

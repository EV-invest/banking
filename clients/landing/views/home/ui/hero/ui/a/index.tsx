import { Text } from "@/shared/ui/text";
import { HeroACanvas } from "./canvas";
import { HeroACta } from "./cta";
import { HeroAStats } from "./stats";

/**
 * Variant A — scroll-zoom metaphor. Server Component.
 *
 * Static copy (heading + body) is rendered here on the server and passed as
 * `children` to the {@link HeroACanvas} client island, which applies zoom
 * transforms without pulling the text into the client bundle. The CTA button
 * and the stats ribbon are also separate islands/components.
 */
export function HeroA() {
  return (
    <section
      id="hero"
      className="relative h-screen flex flex-col justify-center items-center overflow-hidden z-10"
    >
      <HeroACanvas cta={<HeroACta />}>
        <h1 className="text-4xl sm:text-6xl md:text-8xl font-serif-display font-light text-white leading-tight mb-6">
          Invest in{" "}
          <span className="italic text-main-accent-t1 font-serif">Quy Nhon</span>
          <br />
          Through Institutional Vision.
        </h1>
        <Text className="sm:text-base md:text-lg max-w-2xl mx-auto mb-12">
          EV Investment bridges the gap between premium coastal real estate
          development and sophisticated investors. Experience high-yield real
          estate assets in Vietnam&apos;s fastest-growing coastal hub.
        </Text>
      </HeroACanvas>

      <HeroAStats />
    </section>
  );
}

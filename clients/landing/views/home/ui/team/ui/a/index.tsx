import Image from "next/image";
import { Text } from "@/shared/ui/text";
import { Reveal } from "@/shared/ui/reveal";
import { ASSETS } from "@/shared/config/assets";
import { TEAM } from "@/entities/team";
import { Card } from "../shared/cards";
import { TeamPlaceholders } from "../shared/placeholders";

/**
 * Variant A — with {@link Reveal} scroll-reveal animation on both blocks.
 * Server Component; the only client islands are {@link Reveal} (motion) and
 * {@link TeamPlaceholders} (CTA clicks).
 */
export function TeamA() {
  return (
    <section
      id="team"
      className="py-24 relative border-t border-main-mist/10 bg-main-black"
    >
      <div className="container">
        <Reveal className="grid lg:grid-cols-12 gap-12 items-center mb-16">
          <div className="lg:col-span-6 space-y-4">
            <span className="text-xs font-mono-tech text-main-accent-t1 tracking-[0.3em] uppercase block">
              Management &amp; Expertise
            </span>
            <h2 className="text-3xl sm:text-5xl font-serif-display text-white font-light">
              Led by{" "}
              <span className="italic text-main-accent-t1 font-serif">Institutional Pioneers</span>
            </h2>
            <Text className="max-w-xl">
              The EV Investment team combines international experience in
              investment, risk management, and real estate development. TODO
            </Text>
          </div>
          <div className="lg:col-span-6">
            <div className="relative aspect-[16/9] rounded-xl overflow-hidden border border-main-mist/10 shadow-2xl">
              <Image
                src={ASSETS.office_interior}
                alt="EV Investment Boardroom"
                fill
                sizes="(min-width: 1024px) 50vw, 100vw"
                className="object-cover opacity-70"
              />
              <div className="absolute inset-0 bg-gradient-to-t from-main-black/80 via-transparent to-transparent" />
              <div className="absolute bottom-4 left-4 right-4 flex justify-between items-end">
                <div className="space-y-1">
                  <span className="text-[10px] font-mono-tech text-main-accent-t1 uppercase tracking-wider">
                    Head Office
                  </span>
                  <h4 className="text-sm font-bold text-white">EV Boardroom • Quy Nhon</h4>
                </div>
                <Text asChild variant="secondary">
                  <span className="text-[10px] font-mono-tech">Q1 2026</span>
                </Text>
              </div>
            </div>
          </div>
        </Reveal>

        <Reveal delay={0.05} className="grid sm:grid-cols-2 lg:grid-cols-4 gap-8">
          {TEAM.map(member => (
            <Card
              key={member.name}
              heading={member.name}
              headingClassName="text-white"
              sub={
                <p className="text-xs text-main-accent-t1 font-mono-tech mt-1">
                  {member.role}
                </p>
              }
            >
              <Image
                src={member.photo}
                alt={member.name}
                fill
                sizes="(min-width: 1024px) 25vw, (min-width: 640px) 50vw, 100vw"
                className="object-cover transition-transform duration-500 group-hover:scale-105"
              />
              <div className="absolute inset-0 bg-gradient-to-t from-main-black/85 via-transparent to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-300 flex items-end p-6">
                <Text className="text-xs">{member.bio}</Text>
              </div>
            </Card>
          ))}
          <TeamPlaceholders />
        </Reveal>
      </div>
    </section>
  );
}

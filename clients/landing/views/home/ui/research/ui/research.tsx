"use client";

import { useState } from "react";
import { motion, AnimatePresence } from "motion/react";
import { ChevronRight, ArrowUpRight } from "lucide-react";
import { Button } from "@evinvest/uikit";
import { Text } from "@/shared/ui/text";
import { Logo } from "@/shared/ui/logo";
import { Reveal } from "@/shared/ui/reveal";
import { notifyPlaceholder } from "@/shared/lib/utils";

// Report library. The list (left) and the reading pane (right) both render from
// this — switching a card just moves the index, and the pane cross-fades.
const REPORTS = [
  {
    cat: "Macroeconomics",
    title: "Vietnam Coastal Macro Report 2026",
    paneTitle: "Vietnam Coastal Macro Report 2026",
    date: "May 2026",
    quote:
      "Vietnam’s coastal secondary cities are outperforming saturated primary markets, driven by domestic wealth expansion and direct FDI.",
    body: [
      "Our May 2026 analysis indicates a major shift in capital deployment. As Ho Chi Minh City and Hanoi face regulatory and land availability constraints, institutional real estate capital is rapidly relocating to coastal hubs with active infrastructure development.",
      "Quy Nhon stands out due to its unique combination of deep-water port access, high-speed rail connectivity, and a local government aggressively pushing for technological and tourism transformation.",
    ],
  },
  {
    cat: "Urban Planning",
    title: "Quy Nhon Infrastructure Masterplan & Land Valuation",
    paneTitle: "Quy Nhon Infrastructure Masterplan",
    date: "April 2026",
    quote:
      "The expansion of Phu Cat Airport and the Nhon Hoi Economic Zone are creating unprecedented land valuation uplifts in Quy Nhon.",
    body: [
      "This report maps the correlation between infrastructure milestones and land pricing. With the airport expansion nearing completion and the new coastal highway fully operational, travel times have decreased by 40%, directly impacting luxury resort occupancies.",
      "We analyze specific sub-districts poised for the highest capital appreciation over the next 36 months, providing actionable entry points for institutional portfolios.",
    ],
  },
  {
    cat: "Market Analysis",
    title: "Post-Pandemic Tourism & Hospitality Yield Shifts",
    paneTitle: "Post-Pandemic Hospitality Yield Shifts",
    date: "March 2026",
    quote:
      "Yield structures in hospitality assets are shifting from volume-driven models to exclusive, low-density, high-rate private estates.",
    body: [
      "Post-pandemic high-net-worth travelers demand privacy, wellness, and architectural uniqueness. Our research shows that low-density luxury villas in Quy Nhon command a 35% premium in ADR (Average Daily Rate) compared to traditional luxury hotel suites.",
      "We dissect the operating metrics of leading coastal estates to demonstrate how smart design directly drives superior cash-on-cash yields.",
    ],
  },
];

export function ResearchA() {
  const [active, setActive] = useState(0);
  const report = REPORTS[active];

  // 4. RESEARCH SECTION — quiet navy base (same family as the page) with a
  //    faint dot-grid texture so it reads as its own "document / library" zone
  //    via depth rather than a stark color clash.
  return (
    <section
      id="research"
      className="research-texture py-24 text-main-mist relative overflow-hidden border-y border-main-mist/10"
    >
      <div className="container relative z-10">
        <Reveal className="max-w-3xl mb-16">
          <span className="text-xs font-mono-tech text-main-accent-t1 tracking-[0.3em] uppercase block mb-3">
            Academic Rigor
          </span>
          <h2 className="text-3xl sm:text-5xl font-serif-display text-white font-light leading-tight">
            Bridgewater-Grade{" "}
            <span className="italic text-main-accent-t1 font-serif">
              Research &amp; Insights
            </span>
          </h2>
          <Text className="mt-4">
            We believe in deep macroeconomic analysis. Our research team
            produces exhaustive monthly reports on Vietnam’s economic landscape,
            urbanization trends, and Quy Nhon’s real estate cycles.
          </Text>
        </Reveal>

        {/* Research Carousel / Interactive List */}
        <Reveal delay={0.05} className="grid grid-cols-1 lg:grid-cols-3 gap-8">
          {/* Research Selection Menu */}
          <div className="lg:col-span-1 space-y-4">
            {REPORTS.map((r, idx) => (
              <div
                key={idx}
                onClick={() => setActive(idx)}
                className={`p-6 border border-l-2 cursor-pointer transition-all duration-300 ${
                  active === idx
                    ? "research-panel border-main-mist/10 border-l-main-accent-t1 shadow-lg shadow-main-black/50"
                    : "bg-main-card/40 border-main-mist/10 border-l-transparent hover:bg-main-card/70 hover:border-l-main-mist/30"
                }`}
              >
                <span className="text-[10px] font-mono-tech text-main-accent-t1 uppercase tracking-widest block mb-2">
                  {r.cat}
                </span>
                <h4 className="font-serif-display text-lg text-white font-bold mb-3">
                  {r.title}
                </h4>
                <div className="flex justify-between items-center font-mono-tech text-[10px] text-main-mist/40">
                  <span>{r.date}</span>
                  <span className="flex items-center gap-1">
                    Read <ChevronRight className="w-3 h-3" />
                  </span>
                </div>
              </div>
            ))}
          </div>

          {/* Research Preview Content Pane — darker, opaque reading field so the
              dot texture stops at the edge and the dim body text is easy to focus on.
              `.research-panel` adds a soft warm "low sun" from the top-left. */}
          <motion.div
            layout
            transition={{ layout: { duration: 0.4, ease: [0.22, 1, 0.36, 1] } }}
            className="research-panel lg:col-span-2 border border-main-mist/10 shadow-2xl shadow-main-black/60 p-8 sm:p-12 flex flex-col justify-between"
          >
            <div>
              <div className="border-b border-main-mist/10 pb-6 mb-8">
                <span className="text-xs font-mono-tech text-main-accent-t1 uppercase tracking-widest block mb-1">
                  {report.cat}
                </span>
                <h3 className="text-2xl sm:text-3xl font-serif-display text-white font-bold">
                  {report.paneTitle}
                </h3>
              </div>

              <AnimatePresence mode="popLayout">
                <motion.div
                  key={active}
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  transition={{ duration: 0.25, ease: [0.22, 1, 0.36, 1] }}
                  className="prose prose-sm text-main-mist/70 font-light max-w-none space-y-6 leading-relaxed"
                >
                  <p className="font-serif-display italic text-lg text-main-mist/90">
                    &quot;{report.quote}&quot;
                  </p>
                  {report.body.map((p, i) => (
                    <p key={i}>{p}</p>
                  ))}
                </motion.div>
              </AnimatePresence>
            </div>

            <motion.div
              layout="position"
              className="mt-12 pt-6 border-t border-main-mist/10 flex flex-col sm:flex-row justify-between items-center gap-4"
            >
              <div className="flex items-center gap-3">
                <div className="w-12 h-12 rounded-full bg-main-accent-t1/15 border border-main-accent-t1/30 flex items-center justify-center text-main-accent-t1">
                  <Logo className="w-7 h-7" />
                </div>
                <div>
                  <p className="text-xs font-bold text-white">
                    EV Research Department
                  </p>
                  <Text
                    variant="secondary"
                    className="text-[10px] font-mono-tech"
                  >
                    Lead Author: Dr. Nguyen An, Chief Economist
                  </Text>
                </div>
              </div>
              <Button
                className="bg-main-accent-t1 text-main-black hover:bg-main-mist hover:text-main-brand transition-all duration-300 rounded-none font-mono-tech text-xs tracking-wider uppercase py-5 px-6"
                onClick={() => notifyPlaceholder("Download Full Report")}
              >
                Download Full Report <ArrowUpRight className="w-4 h-4 ml-2" />
              </Button>
            </motion.div>
          </motion.div>
        </Reveal>
      </div>
    </section>
  );
}

"use client";

import { useState } from "react";
import { TrendingUp, MapPin, ArrowUpRight } from "lucide-react";
import { Button } from "@evinvest/uikit";
import { Heading, Text } from "@/shared/ui/text";
import { Reveal } from "@/shared/ui/reveal";
import { notifyPlaceholder } from "@/shared/lib/utils";
import { ASSETS } from "@/shared/config/assets";
import { InvestmentCalculator } from "@/features/investment-calculator";

export function PortfolioA() {
  const [activeTab, setActiveTab] = useState("all");

  // 3. PORTFOLIO SECTION (Bento Grid, DWF Labs & Binance Inspired)
  return (
    <section
      id="portfolio"
      className="py-24 relative border-t border-main-mist/10"
    >
      <div className="container">
        <Reveal className="flex flex-col md:flex-row md:items-end justify-between mb-16">
          <div>
            <span className="text-xs font-mono-tech text-main-accent-t1 tracking-[0.3em] uppercase block mb-3">
              Investment Scope
            </span>
            <h2 className="text-3xl sm:text-5xl font-serif-display text-white font-light">
              Premium Asset{" "}
              <span className="italic text-main-accent-t1 font-serif">Portfolio</span>
            </h2>
          </div>
          <Text className="max-w-md mt-4 md:mt-0">
            Curated, premium, high-yield developments across Quy Nhon city,
            focusing on high appreciation seaside villas and urban luxury
            residences.
          </Text>
        </Reveal>

        {/* Filter Tabs (Binance/DWF style) */}
        <div className="flex flex-wrap gap-2 mb-12 border-b border-main-mist/10 pb-4 font-mono-tech text-xs tracking-wider">
          {["all", "villas", "commercial", "land"].map(tab => (
            <button
              key={tab}
              onClick={() => setActiveTab(tab)}
              className={`px-5 py-2.5 uppercase transition-all duration-300 ${
                activeTab === tab
                  ? "bg-main-accent-t1 text-main-black font-bold"
                  : "text-main-mist/60 hover:text-white hover:bg-main-mist/5"
              }`}
            >
              {tab}
            </button>
          ))}
        </div>

        {/* Bento Grid Layout */}
        <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
          {/* Main Featured Asset (Large Box) */}
          <div className="md:col-span-2 relative overflow-hidden group border border-main-mist/10 bg-main-black/40 flex flex-col justify-end min-h-[450px]">
            <div
              className="absolute inset-0 z-0 bg-cover bg-center transition-transform duration-700 group-hover:scale-105"
              style={{
                backgroundImage: `linear-gradient(to top, rgba(7, 13, 24, 0.96) 10%, rgba(7, 13, 24, 0.2)), url(${ASSETS.luxury_villa})`,
              }}
            />
            <div className="absolute top-4 right-4 bg-main-accent-t1 text-main-black font-mono-tech text-[10px] tracking-widest uppercase px-3 py-1.5 font-bold">
              Featured Deal
            </div>
            <div className="relative z-10 p-8">
              <div className="flex items-center gap-2 text-main-accent-t1 font-mono-tech text-xs mb-3">
                <MapPin className="w-3.5 h-3.5" /> Nhon Ly Beach, Quy Nhon
              </div>
              <Heading scale="main">The Horizon Premium Villas</Heading>
              <Text className="max-w-xl mb-6">
                Exclusive ultra-luxury oceanfront villas with private pools,
                nestled between pristine limestone cliffs and crystal-clear
                turquoise waters.
              </Text>
              <div className="grid grid-cols-3 gap-4 border-t border-main-mist/10 pt-6 max-w-md">
                <div>
                  <Text asChild variant="secondary">
                    <span className="text-[10px] font-mono-tech uppercase block mb-1">
                      Target Yield
                    </span>
                  </Text>
                  <span className="text-lg font-serif-display text-main-accent-t2 font-bold">
                    12.5% p.a.
                  </span>
                </div>
                <div>
                  <Text asChild variant="secondary">
                    <span className="text-[10px] font-mono-tech uppercase block mb-1">
                      Appreciation
                    </span>
                  </Text>
                  <span className="text-lg font-serif-display text-main-accent-t3 font-bold">
                    18% YoY
                  </span>
                  {/* rare gold highlight */}
                </div>
                <div>
                  <Text asChild variant="secondary">
                    <span className="text-[10px] font-mono-tech uppercase block mb-1">
                      Status
                    </span>
                  </Text>
                  <span className="text-lg font-serif-display text-white font-bold">
                    Pre-Launch
                  </span>
                </div>
              </div>
            </div>
          </div>

          {/* Side Asset 1 */}
          <div className="relative overflow-hidden group border border-main-mist/10 bg-main-black/40 flex flex-col justify-end min-h-[450px]">
            <div
              className="absolute inset-0 z-0 bg-cover bg-center transition-transform duration-700 group-hover:scale-105"
              style={{
                backgroundImage: `linear-gradient(to top, rgba(7, 13, 24, 0.96) 20%, rgba(7, 13, 24, 0.4)), url(${ASSETS.quynhon_future})`,
              }}
            />
            <div className="relative z-10 p-8">
              <div className="flex items-center gap-2 text-main-accent-t1 font-mono-tech text-xs mb-3">
                <MapPin className="w-3.5 h-3.5" /> Quy Nhon Center
              </div>
              <Heading scale="alt">Quy Nhon Bay Residences</Heading>
              <Text className="mb-6">
                Premium high-rise apartments with panoramic views of the bay,
                integrating luxury amenities and smart-home technology.
              </Text>
              <div className="flex justify-between items-center border-t border-main-mist/10 pt-6">
                <div>
                  <Text asChild variant="secondary">
                    <span className="text-[9px] font-mono-tech uppercase block mb-0.5">
                      LTV Ratio
                    </span>
                  </Text>
                  <span className="text-sm font-serif-display text-white font-bold">
                    55% Max
                  </span>
                </div>
                <Button
                  variant="ghost"
                  className="p-0 text-main-accent-t1 hover:text-white hover:bg-transparent font-mono-tech text-xs tracking-wider"
                  onClick={() => notifyPlaceholder("View Deal Sheet")}
                >
                  Deal Sheet <ArrowUpRight className="w-3.5 h-3.5 ml-1" />
                </Button>
              </div>
            </div>
          </div>

          {/* Asset 3 (Modular Bento Grid item - Info heavy like Binance) */}
          <div className="border border-main-mist/10 bg-main-card p-8 flex flex-col justify-between">
            <div>
              <div className="inline-flex items-center gap-1.5 px-2 py-1 bg-main-accent-t1/10 text-main-accent-t1 border border-main-accent-t1/20 text-[9px] font-mono-tech uppercase tracking-wider mb-6">
                <TrendingUp className="w-3 h-3" /> Market Growth
              </div>
              <Heading scale="alt">Why Quy Nhon?</Heading>
              <Text className="mb-6">
                Positioned as the new gateway of Central Vietnam, Quy Nhon is
                undergoing a multi-billion dollar infrastructure upgrade,
                transforming into a global science and beach tourism
                destination.
              </Text>
            </div>
            <ul className="space-y-3 border-t border-main-mist/10 pt-6 font-mono-tech text-xs">
              <li className="flex justify-between">
                <Text asChild variant="secondary">
                  <span>Infrastructure Investment:</span>
                </Text>
                <span className="text-white font-bold">$2.4 Billion</span>
              </li>
              <li className="flex justify-between">
                <Text asChild variant="secondary">
                  <span>Tourism Growth Rate:</span>
                </Text>
                <span className="text-main-accent-t2 font-bold">+28% YoY</span>
              </li>
              <li className="flex justify-between">
                <Text asChild variant="secondary">
                  <span>FDI Inflow (2025):</span>
                </Text>
                <span className="text-main-acc    ent-t2 font-bold">$420M</span>
              </li>
            </ul>
          </div>

          {/* Asset 4 — the investment calculator lives in its own feature. */}
          <InvestmentCalculator className="md:col-span-2" />
        </div>
      </div>
    </section>
  );
}

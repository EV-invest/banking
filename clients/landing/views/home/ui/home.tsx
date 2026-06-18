import { Hero } from "./hero";
import { Portfolio } from "./portfolio";
import { Research } from "./research";
import { Team } from "./team";

// Server Component composing the page top-to-bottom. It renders sections
// uniformly and is intentionally UNAWARE that any of them are A/B tested — each
// section owns its own variant resolution (see hero/ui/hero.tsx,
// team/ui/team.tsx). Header and Footer are page chrome rendered by the
// application layout, not page sections.
export function HomeView() {
  return (
    <div className="min-h-screen bg-main-black text-main-mist font-sans">
      <Hero />
      <Portfolio />
      <Research />
      <Team />
    </div>
  );
}

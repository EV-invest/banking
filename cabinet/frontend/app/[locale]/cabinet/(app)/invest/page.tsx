import { Suspense } from "react";

import { I18nScope } from "@evinvest/i18n/react";

import { readBookPolicy } from "@/entities/book/api/book-server";
import { readAllocations, readFeePolicy, readFundNav, readPositions, readRedemptions } from "@/entities/fund/api/fund-server";
import { readWallet } from "@/entities/wallet/api/wallet-server";
import type { ServerRead } from "@/shared/api/server/bff";
import type { AllocationList, PositionList } from "@/shared/contracts";
import { InvestView } from "@/views/invest/ui/invest-view";
import type { ProductCardSeed } from "@/views/invest/ui/product-card";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// The fund-shares surface: the portfolio summary and one row per product. Dealing moved
// to `/invest/[service]` — a subscription is a decision about one fund, and a form under
// every row made the list read as a stack of forms rather than as a portfolio.
//
// Read on the server the way /wallet is, and never waited for: the fallback is the same
// view in its loading state, flushed at once. The lists stream in first; each card's NAV,
// terms and book are asked for as soon as the catalog and the positions name the products,
// and follow as a second step under each card's own boundary — the same two steps the
// browser takes, minus the page's JavaScript between them, and the summary is never held
// back by the second. A read that fails or times out arrives as no seed and its reader
// makes the browser read it always made; the KYC gate on each card's action stays a browser
// read (`/kyc/status` is not the BFF's).
export default async function InvestPage() {
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/invest/page.tsx")}>
      <Suspense fallback={<InvestView />}>
        <SeededInvest />
      </Suspense>
    </I18nScope>
  );
}

async function SeededInvest() {
  const catalogRead = readAllocations();
  const positionsRead = readPositions();
  // Handed over unresolved: it streams to the cards after the lists, not ahead of them.
  const cards = Promise.all([catalogRead, positionsRead]).then(([catalog, positions]) => readCards(servicesOf(catalog, positions)));
  const [catalog, positions, redemptions, wallet] = await Promise.all([catalogRead, positionsRead, readRedemptions(), readWallet()]);
  return <InvestView initial={{ catalog: catalog ?? undefined, positions: positions ?? undefined, redemptions: redemptions ?? undefined, wallet: wallet ?? undefined, cards }} />;
}

/** Every product the view draws a card for: the open catalog plus anything still held. */
function servicesOf(catalog: ServerRead<AllocationList> | null, positions: ServerRead<PositionList> | null): string[] {
  const services = new Set<string>();
  for (const allocation of catalog?.data.allocations ?? []) services.add(allocation.service);
  for (const position of positions?.data.positions ?? []) if (position.service) services.add(position.service);
  return [...services];
}

async function readCards(services: string[]): Promise<Record<string, ProductCardSeed>> {
  const cards = await Promise.all(
    services.map(async (service) => {
      const [nav, fee, book] = await Promise.all([readFundNav(service), readFeePolicy(service), readBookPolicy(service)]);
      const seed: ProductCardSeed = { nav: nav ?? undefined, fee: fee ?? undefined, book: book ?? undefined };
      return [service, seed] as const;
    }),
  );
  return Object.fromEntries(cards);
}

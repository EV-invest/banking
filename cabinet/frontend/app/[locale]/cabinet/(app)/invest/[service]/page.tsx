import { Suspense } from "react";

import { I18nScope } from "@evinvest/i18n/react";

import { readBookPolicy } from "@/entities/book/api/book-server";
import { readAccruedFees, readAllocation, readFeePolicy, readFundNav, readPositions, readRedemptions } from "@/entities/fund/api/fund-server";
import { ProductView } from "@/views/invest/ui/product-view";
import { routeMessages } from "@/shared/config/i18n";
import { currentLocale } from "@/shared/config/locale";

// One product per page. The slug is the allocation's `service` id — the same key every
// fund RPC takes — so the URL is the product's identity rather than an index into a list
// that reorders. An unregistered slug renders the not-found state; the hub refuses it
// too, so a wrong link can never become a surface that deals.
//
// Read on the server the way /wallet is, and never waited for: the fallback is the same
// view in its loading state, flushed at once. A read that fails or times out — a 404 for an
// unregistered slug among them — arrives as no seed, and the view makes the browser read it
// always made, which is also what tells "not registered" from "not reachable". The KYC gate
// on Subscribe stays a browser read (`/kyc/status` is not the BFF's).
export default async function ProductPage({ params }: { params: Promise<{ service: string }> }) {
  const { service: slug } = await params;
  const service = decodeURIComponent(slug);
  const locale = await currentLocale();
  return (
    <I18nScope messages={routeMessages(locale, "app/[locale]/cabinet/(app)/invest/[service]/page.tsx")}>
      <Suspense fallback={<ProductView service={service} />}>
        <SeededProduct service={service} />
      </Suspense>
    </I18nScope>
  );
}

async function SeededProduct({ service }: { service: string }) {
  const [detail, positions, nav, redemptions, fee, accrued, book] = await Promise.all([
    readAllocation(service),
    readPositions(),
    readFundNav(service),
    readRedemptions(),
    readFeePolicy(service),
    readAccruedFees(service),
    readBookPolicy(service),
  ]);
  return (
    <ProductView
      service={service}
      initial={{
        detail: detail ?? undefined,
        positions: positions ?? undefined,
        nav: nav ?? undefined,
        redemptions: redemptions ?? undefined,
        fee: fee ?? undefined,
        accrued: accrued ?? undefined,
        book: book ?? undefined,
      }}
    />
  );
}

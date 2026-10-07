// Stand-in for `../chart-feed.ts` in `use-candle-chart.test.ts`. The real module's REST
// half imports `shared/lib/api-client.ts`, which Node's type stripping cannot load; the hook
// under test takes only `historyWindow` from it, and the tests hand it a fake `ChartFeed`
// whose history ignores the window. So the window here is the real one's shape, built from
// the same `historyFrom`.

import type { CandleResolution } from "../../../../shared/contracts/book.ts";
import { historyFrom } from "../candles.ts";

export function historyWindow(resolution: CandleResolution, now = Math.floor(Date.now() / 1000)): { from: number; to: number } {
  return { from: historyFrom(now, resolution), to: now };
}

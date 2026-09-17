// The admin console's treatment on top of uikit `Table`, which carries the borders, the
// row hover, the scroll wrapper and a `p-2` cell. Two things stay the console's own.

/** The tracked-uppercase header every admin list has always worn (`views/trade/ui/fills-table.tsx`
 *  is the investor-side counterpart, without the tracking). */
export const TABLE_HEAD = "text-xs font-medium uppercase tracking-wide text-ink-soft";

/** Cell padding for a table that sits edge-to-edge in a `p-0` card: the kit's `p-2` assumes a
 *  padded surface around it, and a figure two pixels off a card edge reads as clipped. */
export const EDGE_CELL = "px-5 py-3";

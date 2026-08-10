-- 0022: an allocation now carries an authorised unit supply.
--
-- 0021 answered "does this product exist and is it open?". It did not answer "how much
-- of it is there?" — an open fund would mint units forever, so a product sized for a
-- thousand shares and one sized for a hundred million were the same object with
-- different marketing. This column is the second gate on the subscribe path: the mint
-- is refused once `units_outstanding + minted` would pass the cap.
--
-- Control plane only, like the rest of this table — ZERO amounts move here. The
-- authoritative issued supply stays in TigerBeetle (`shares_outstanding:<service>`);
-- this is the ceiling it is checked against.

-- 18-dp base units as a digit string, matching `fund_valuations.units_outstanding` and
-- `fund_positions` — the one representation the whole codebase uses for a `Shares`
-- amount, so a cap and the supply it bounds are always comparable without conversion.
-- `> 0` mirrors `Allocation::set_unit_cap`: a zero cap would be indistinguishable from
-- an unset one while silently refusing every subscription, so stopping new money is
-- `state = 'closed'`, which says so.
ALTER TABLE allocations
    ADD COLUMN unit_cap TEXT NOT NULL DEFAULT '100000000000000000000000000'
        CHECK (unit_cap ~ '^[1-9][0-9]*$');

-- The default is `DEFAULT_UNIT_CAP` (100,000,000 units × 10^18) — high enough that it
-- cannot surprise the products 0021 backfilled, finite so the registry always has a
-- number to show. An operator narrows it per product before opening.
COMMENT ON COLUMN allocations.unit_cap IS
    'Authorised unit supply in 18-dp base units. Subscribe refuses to mint past it; redemptions are unaffected.';

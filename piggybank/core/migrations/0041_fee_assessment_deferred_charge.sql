-- 0041: a fee assessment may collect nothing and still be a fact.
--
-- `fee_assessments.charged_units` (0023) was born positive-only, on the premise that a
-- charge which collected nothing was not an assessment and would never be written. That
-- premise let a holder skip their fee: with every unit of a position escrowed by a
-- resting sell on the book (0037) or reserved by a queued redemption, the clawback's cap
-- was zero, nothing was recorded, `fees_accrued_at` never moved, and the fee stayed
-- deferred for exactly as long as the ask rested — then landed as one lump the day the
-- order came off. The rule is now that a fee accrues on the WHOLE position and is
-- collected from what the holding can spare, the shortfall carried in
-- `fund_positions.fee_debt` (0023). A charge that defers entirely is therefore recorded
-- with `charged_units = '0'`, so the debt it opened and the clock it moved have their
-- audit row like every other charge.
--
-- Same WIDENING of a CHECK as 0029/0031: DROP + ADD, no table rewrite, no rows to
-- validate against a stricter rule (every existing row already satisfies the wider one).
-- Backward compatible in both directions of a rolling deploy: a pod predating this file
-- only ever writes positive values, which the new constraint accepts, and reads every row
-- the new pod writes — its own domain code refuses to record a zero charge, so it never
-- tries to. Reversible while no zero row exists (re-add the `'^[1-9][0-9]*$'` CHECK);
-- once one does, the pre-0041 binary still reads it as a legitimate row: the audit
-- readers parse `charged_units` as an unsigned number and the operation feed already
-- labels a shortfall `partly_deferred`.
--
-- Numbered 0041 because 0039 (allocation backing) and 0040 (book acknowledgement) are
-- taken by the in-flight #271 stack; sqlx applies by version, so the gap is harmless
-- until they land and closes when they do.
--
-- Migrations run at hub boot.
-- LOCAL: scoped to this migration's transaction, never left on the pool connection.
SET LOCAL lock_timeout = '3s';

ALTER TABLE fee_assessments DROP CONSTRAINT fee_assessments_charged_units_check;
ALTER TABLE fee_assessments ADD CONSTRAINT fee_assessments_charged_units_check CHECK (charged_units ~ '^[0-9]+$');

COMMENT ON COLUMN fee_assessments.charged_units IS
    'Units clawed back by this charge, in 18-dp base units. ''0'' when the whole charge deferred into fund_positions.fee_debt because every unit was escrowed or reserved.';

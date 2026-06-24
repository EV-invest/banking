-- 0009: park outbox events into a distinct terminal state instead of dropping them.
--
-- Before this, the relay called `mark_dispatched` on a non-retryable failure, so a
-- parked (money-moving) event was indistinguishable from a successfully-applied one —
-- silently and permanently lost from the active pipeline, recoverable only by reading
-- `last_error` by hand. A parked event must stay QUERYABLE (so reconciliation/the reaper
-- can find it and an operator can resolve it) yet EXCLUDED from the normal drain (so one
-- bad event can't wedge the single-worker queue).
--
-- `parked_at` is that distinct terminal state: set on a non-retryable failure, never
-- alongside `dispatched_at`. The drain predicate becomes
-- `dispatched_at IS NULL AND parked_at IS NULL`; the partial index follows so the planner
-- still uses it. A `compensated_at` stamp records that a parked multi-leg event has had a
-- compensating event emitted (the aggregate flipped to Failed), so reconciliation can tell
-- an unresolved park from one already routed to its recovery path.
ALTER TABLE outbox ADD COLUMN parked_at TIMESTAMPTZ;
ALTER TABLE outbox ADD COLUMN compensated_at TIMESTAMPTZ;

DROP INDEX outbox_undispatched_idx;
CREATE INDEX outbox_undispatched_idx ON outbox (seq) WHERE dispatched_at IS NULL AND parked_at IS NULL;

-- Reconciliation and the reaper scan parked rows by recency; index the terminal stamp.
CREATE INDEX outbox_parked_idx ON outbox (parked_at) WHERE parked_at IS NOT NULL;

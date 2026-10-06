-- 0050: the concrete `bank:*` permissions concierge resolves for a user's seat, a snapshot
-- on every lifecycle row like the role. NULL: concierge has not said yet.
--
-- `bridge_deferred_event` carries it like every other field `bridge::apply` reads, or a
-- parked row would replay as "holds nothing".
--
-- REVERSIBILITY: ALTER TABLE users DROP COLUMN permissions;
--                ALTER TABLE bridge_deferred_event DROP COLUMN permissions;
SET LOCAL lock_timeout = '3s';

ALTER TABLE users ADD COLUMN permissions TEXT[];
ALTER TABLE bridge_deferred_event ADD COLUMN permissions TEXT[] NOT NULL DEFAULT '{}';

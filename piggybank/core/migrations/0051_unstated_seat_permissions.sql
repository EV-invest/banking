-- 0051: a parked event keeps whether concierge STATED a seat's `bank:*` set. NULL: it did
-- not (`seat_permissions` absent), and the replay must leave the mirror alone; '{}' is a
-- stated empty set and must be applied. Rows parked before this keep the '{}' they were
-- given: field 12 could not tell the two apart.
--
-- REVERSIBILITY: UPDATE bridge_deferred_event SET permissions = '{}' WHERE permissions IS NULL;
--                ALTER TABLE bridge_deferred_event ALTER COLUMN permissions SET NOT NULL,
--                  ALTER COLUMN permissions SET DEFAULT '{}';
SET LOCAL lock_timeout = '3s';

ALTER TABLE bridge_deferred_event ALTER COLUMN permissions DROP NOT NULL, ALTER COLUMN permissions DROP DEFAULT;

-- 0042: the operator's acknowledgement of holders who could not be told (#264).
--
-- Since 0036 a scheduled change that TIGHTENS the terms does not promote while a holder's
-- notice is undelivered. Right for a relay outage; a dead end for a holder the identity
-- plane can never reach (an unverified mailbox, no mirrored id): every notice to them is
-- retired within minutes of scheduling, and the product can never tighten its terms, not
-- even through an owner consilium. The way out is explicit and on the record: an operator
-- entitled to the change ACKNOWLEDGES the undelivered notices, and the change binds over
-- exactly the holders named here — `promote` still refuses over anyone else.
--
-- Expand only: three nullable columns, nothing rewritten, and a row written by a binary
-- that does not know them reads as "not acknowledged", which is the safe reading.

SET lock_timeout = '3s';

ALTER TABLE fee_policy_changes
    -- Who took responsibility, when, and for whom: the banking ids of the holders whose
    -- notice had not been delivered at the moment of the acknowledgement.
    ADD COLUMN notices_waived_by    TEXT,
    ADD COLUMN notices_waived_at    TIMESTAMPTZ,
    ADD COLUMN notices_waived_users UUID[];

-- An acknowledgement is whole or absent: a name without a moment, or a moment without the
-- holders it covered, is not an audit record.
ALTER TABLE fee_policy_changes ADD CONSTRAINT fee_policy_change_notice_waiver_is_whole
    CHECK (num_nonnulls(notices_waived_by, notices_waived_at, notices_waived_users) IN (0, 3));
-- It names at least one holder: "nothing to acknowledge" is refused by the write path, and
-- the schema says so too, so the history never shows a waiver that covered nobody.
ALTER TABLE fee_policy_changes ADD CONSTRAINT fee_policy_change_notice_waiver_names_someone
    CHECK (notices_waived_users IS NULL OR cardinality(notices_waived_users) > 0);
-- Notices exist only once the change is scheduled, so an acknowledgement does too.
ALTER TABLE fee_policy_changes ADD CONSTRAINT fee_policy_change_notice_waiver_follows_scheduling
    CHECK (notices_waived_at IS NULL OR scheduled_at IS NOT NULL);

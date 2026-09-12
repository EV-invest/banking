-- The governance mail queue learns to announce a PAYMENT, and the two payment mail kinds.
--
-- WHY THE SAME QUEUE. `consilium_mail` is the one route by which any governance mail leaves
-- this plane: written in the same transaction as the fact it announces, drained by the
-- singleton worker into concierge's relay, idempotent by `dedupe_key`. A payment's consent
-- mail needs exactly those three properties — an order committed with no record of the
-- message that asks its subject is the same silent stall an unmailed consilium is — so it
-- rides the same queue rather than a second one with a second worker to keep honest.
--
-- WHY `consilium_id` GOES NULLABLE RATHER THAN A SECOND TABLE. A row is ABOUT exactly one
-- subject: the consilium it announces or asks about, or the payment whose subject is being
-- asked to consent. Two nullable FKs with `num_nonnulls(...) = 1` state that to the database;
-- a mail with both, or neither, cannot be written. Every existing row names a consilium, so
-- the constraint validates instantly.
--
-- The kind CHECK is the inline one Postgres named `consilium_mail_kind_check`, so widening
-- it is DROP + ADD; no row of either new kind exists yet.

SET lock_timeout = '3s';

ALTER TABLE consilium_mail ALTER COLUMN consilium_id DROP NOT NULL;
ALTER TABLE consilium_mail ADD COLUMN payment_id UUID REFERENCES payments (id) ON DELETE CASCADE;
ALTER TABLE consilium_mail ADD CONSTRAINT consilium_mail_names_one_subject CHECK (num_nonnulls(consilium_id, payment_id) = 1);
ALTER TABLE consilium_mail DROP CONSTRAINT consilium_mail_kind_check;
ALTER TABLE consilium_mail ADD CONSTRAINT consilium_mail_kind_check
    CHECK (kind IN ('payout_approval', 'payout_outcome', 'token_burned', 'payment_consent', 'payment_approval'));

-- The worker's other lookup: which payment a delivered consent mail belongs to, so the seat's
-- `notified` flag can be flipped without scanning the queue.
CREATE INDEX consilium_mail_payment_idx ON consilium_mail (payment_id) WHERE payment_id IS NOT NULL;

-- `revenue` is a PARTY (a payment can pay the fund's earned money out) but never a deposit
-- target: money arriving from outside is attested by a chain watcher and lands on the party
-- whose address received it, and no address is the fee claim's. `application::balance`
-- already refuses it; this makes the refusal the database's statement rather than one
-- adapter's habit, so a second writer of `deposits` cannot credit revenue by mistake.
ALTER TABLE deposits ADD CONSTRAINT deposits_party_is_never_revenue CHECK (party_kind <> 'revenue');

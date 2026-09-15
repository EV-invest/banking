-- 0043: a governance mail WITHDRAWN before it was delivered (#319).
--
-- A holder's notice is written in the transaction that schedules the change, and it
-- leaves through the queue whenever the relay is up. Cancel the change while the relay is
-- down and the notice is still queued; the relay coming back then tells every holder that
-- terms which will never bind "change on <date>". A withdrawn row is terminal: the worker
-- never picks it up, and it is not a notice anybody is still owed.
--
-- WHY A COLUMN AND NOT THE ATTEMPT CEILING. A row pinned at the ceiling is one the mailer
-- GAVE UP on — an alert, and the figure the operator's acknowledgement (0042) is offered
-- over. A withdrawn notice is neither: nobody failed to reach anybody, the fact was taken
-- back. The audit trail has to keep the two apart.
--
-- Expand only: one nullable column, nothing rewritten. A row written by a binary that does
-- not know the column reads as "not withdrawn". A worker that does not know it would still
-- deliver a withdrawn row — the mailer runs as a singleton, so that window is the one
-- restart that swaps the binary.

SET lock_timeout = '3s';

ALTER TABLE consilium_mail ADD COLUMN withdrawn_at TIMESTAMPTZ;

-- A delivered mail cannot be taken back, and a withdrawn one is never delivered.
ALTER TABLE consilium_mail ADD CONSTRAINT consilium_mail_withdrawn_is_unsent
    CHECK (sent_at IS NULL OR withdrawn_at IS NULL);

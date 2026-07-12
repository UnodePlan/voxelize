ALTER TABLE match_participants
    ADD COLUMN enqueued_at TIMESTAMPTZ;

UPDATE match_participants AS participant
SET enqueued_at = match.created_at
FROM matches AS match
WHERE participant.match_id = match.id
  AND participant.enqueued_at IS NULL;

ALTER TABLE match_participants
    ALTER COLUMN enqueued_at SET NOT NULL;

CREATE INDEX match_participants_queue_order_idx
    ON match_participants (enqueued_at, seat_id)
    WHERE state IN ('waiting', 'preparing');

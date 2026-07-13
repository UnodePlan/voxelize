ALTER TABLE match_participants
    ADD COLUMN terminal_cause TEXT,
    ADD COLUMN terminal_at TIMESTAMPTZ,
    ADD COLUMN survived_ms BIGINT;

ALTER TABLE match_participants
    ADD CONSTRAINT match_participants_terminal_cause_check
        CHECK (terminal_cause IS NULL OR terminal_cause IN (
            'melee', 'reconnect_timeout', 'hard_deadline'
        )),
    ADD CONSTRAINT match_participants_survived_ms_check
        CHECK (survived_ms IS NULL OR survived_ms BETWEEN 0 AND 4294967295),
    ADD CONSTRAINT match_participants_terminal_result_shape_check
        CHECK (
            (terminal_cause IS NULL AND terminal_at IS NULL AND survived_ms IS NULL)
            OR
            (terminal_cause IS NOT NULL AND terminal_at IS NOT NULL AND survived_ms IS NOT NULL)
        );

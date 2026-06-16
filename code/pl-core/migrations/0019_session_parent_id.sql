ALTER TABLE sessions
    ADD COLUMN parent_session_id TEXT;

UPDATE sessions
    SET parent_session_id = (
        SELECT origin_session_id
        FROM session_handoffs
        WHERE session_handoffs.target_session_id = sessions.id
        LIMIT 1
    )
    WHERE EXISTS (
        SELECT 1
        FROM session_handoffs
        WHERE session_handoffs.target_session_id = sessions.id
    );

UPDATE sessions
    SET visibility = 'active'
    WHERE visibility = 'handoffOrigin';

CREATE INDEX IF NOT EXISTS idx_sessions_parent_session
    ON sessions(parent_session_id);

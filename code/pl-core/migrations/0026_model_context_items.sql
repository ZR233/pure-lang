ALTER TABLE messages
    ADD COLUMN item_type TEXT NOT NULL DEFAULT 'message';

CREATE INDEX idx_messages_session_item_type_created_at
    ON messages(session_id, item_type, created_at ASC, id ASC);

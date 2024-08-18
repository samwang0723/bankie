CREATE TABLE outbox (
    id SERIAL PRIMARY KEY,
    transcaction_id UUID NOT NULL,
    event_type VARCHAR(255) NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    processed_at TIMESTAMP,
    processed BOOLEAN NOT NULL DEFAULT FALSE
);

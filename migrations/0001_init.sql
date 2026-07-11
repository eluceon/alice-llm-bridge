-- The application role owns its database, so creating a schema here needs
-- no superuser rights; only role/database provisioning does (see
-- docker/postgres/initdb).
CREATE SCHEMA IF NOT EXISTS bridge;

CREATE TABLE bridge.messages (
    id BIGSERIAL PRIMARY KEY,
    profile TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content TEXT NOT NULL,
    model TEXT,
    prompt_tokens INT,
    completion_tokens INT,
    cost_micros BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX messages_profile_id_idx ON bridge.messages (profile, id);
CREATE INDEX messages_created_at_idx ON bridge.messages (created_at);

CREATE TABLE bridge.summaries (
    profile TEXT PRIMARY KEY,
    content TEXT NOT NULL,
    covers_until_message_id BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

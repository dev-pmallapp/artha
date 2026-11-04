-- namespace: core   version: 1   name: sessions
-- Chat sessions, their message history, and a generic namespaced key-value table.
-- Forward-only. NEVER edit this file after release: its SHA-256 is recorded in
-- _artha_migrations and a mismatch aborts startup. Add 0002_*.sql instead.

CREATE TABLE core_session (
    id          TEXT    PRIMARY KEY NOT NULL,              -- UUIDv7, lowercase hyphenated
    title       TEXT,
    workdir     TEXT    NOT NULL,                          -- canonical launch dir (drives --continue)
    mode        TEXT    NOT NULL CHECK (mode IN ('local', 'cloud')),
    provider    TEXT,                                      -- 'anthropic-messages' | 'openai-chat' | 'cloud'
    model       TEXT,
    parent_id   TEXT    REFERENCES core_session(id) ON DELETE SET NULL,   -- forked-from
    archived    INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    metadata    TEXT    NOT NULL DEFAULT '{}' CHECK (json_valid(metadata)),
    created_at  INTEGER NOT NULL,                          -- epoch ms UTC
    updated_at  INTEGER NOT NULL
) STRICT;

CREATE INDEX core_session_by_workdir ON core_session (workdir, archived, updated_at DESC);

CREATE TABLE core_message (
    id             INTEGER PRIMARY KEY,                    -- rowid alias
    session_id     TEXT    NOT NULL REFERENCES core_session(id) ON DELETE CASCADE,
    seq            INTEGER NOT NULL CHECK (seq >= 0),      -- 0-based, dense per session
    role           TEXT    NOT NULL CHECK (role IN ('system', 'user', 'assistant', 'tool')),
    content        TEXT    NOT NULL CHECK (json_valid(content)),  -- opaque to artha-data; artha-core's message JSON
    input_tokens   INTEGER,
    output_tokens  INTEGER,
    created_at     INTEGER NOT NULL,
    UNIQUE (session_id, seq)
) STRICT;

CREATE TABLE core_kv (
    namespace   TEXT    NOT NULL,                          -- owning crate/tool, e.g. 'tui', 'memory'
    key         TEXT    NOT NULL,
    value       TEXT    NOT NULL CHECK (json_valid(value)),
    updated_at  INTEGER NOT NULL,
    PRIMARY KEY (namespace, key)
) STRICT, WITHOUT ROWID;

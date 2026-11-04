-- namespace: core   version: 2   name: checkpoint_links
-- Links checkpoint commits (which live in the per-workdir git repo) to the
-- conversation position that produced them, so "rewind to message N" can
-- restore files and truncate history together. The commit itself remains the
-- source of truth for file content; this table is an index and may be rebuilt
-- from commit-message trailers.

CREATE TABLE core_checkpoint_link (
    commit_oid    TEXT    PRIMARY KEY NOT NULL,            -- 40-hex git object id
    workdir_hash  TEXT    NOT NULL,                        -- snapshot/<hash> directory name
    session_id    TEXT    REFERENCES core_session(id) ON DELETE SET NULL,
    message_seq   INTEGER,                                 -- core_message.seq the edit belongs to
    tool_call_id  TEXT,
    kind          TEXT    NOT NULL CHECK (kind IN ('pre_edit', 'post_edit', 'pre_restore')),
    created_at    INTEGER NOT NULL
) STRICT;

CREATE INDEX core_checkpoint_link_by_session ON core_checkpoint_link (session_id, message_seq);

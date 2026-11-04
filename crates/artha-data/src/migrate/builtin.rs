//! Migrations for the `core` namespace — the only tables `artha-data` owns:
//! `core_session`, `core_message`, `core_kv`, `core_checkpoint_link`.
//! SQL lives in `crates/artha-data/migrations/core/`.

use super::{Migration, MigrationSet};

/// Always applied first by [`crate::Db::open`].
pub static CORE_MIGRATIONS: MigrationSet = MigrationSet::new(
    "core",
    &[
        Migration {
            version: 1,
            name: "sessions",
            sql: include_str!("../../migrations/core/0001_sessions.sql"),
        },
        Migration {
            version: 2,
            name: "checkpoint_links",
            sql: include_str!("../../migrations/core/0002_checkpoint_links.sql"),
        },
    ],
);

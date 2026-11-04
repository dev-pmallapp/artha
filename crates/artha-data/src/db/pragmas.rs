//! Per-connection SQLite configuration, applied once right after open.
//!
//! | pragma                 | value      | why                                                    |
//! |------------------------|------------|--------------------------------------------------------|
//! | `journal_mode`         | `WAL`      | concurrent readers + one writer across processes       |
//! | `synchronous`          | `NORMAL`   | durable at checkpoint, safe with WAL, much faster     |
//! | `foreign_keys`         | `ON`       | enforce `REFERENCES` (off by default in SQLite)        |
//! | `busy_timeout`         | options    | wait instead of failing with `SQLITE_BUSY`             |
//! | `temp_store`           | `MEMORY`   | no temp files under `~/.artha/data`                    |
//! | `trusted_schema`       | `OFF`      | defense-in-depth against crafted schema SQL            |
//!
//! `journal_mode=WAL` is skipped for in-memory databases (unsupported).

use rusqlite::Connection;

use crate::db::DbOptions;
use crate::Result;

/// Apply the table above to a freshly opened connection.
pub(crate) fn apply(conn: &Connection, options: &DbOptions, in_memory: bool) -> Result<()> {
    todo!()
}

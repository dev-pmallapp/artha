//! Small shared value types used across stores.
//!
//! Timestamps are stored as `INTEGER` Unix epoch **milliseconds (UTC)** in
//! SQLite and as the same integer in JSON. This avoids pulling a date-time
//! crate into the data layer; callers that need calendars (`artha-jobs` cron,
//! TUI rendering) convert at their boundary.

use rusqlite::types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};

/// Milliseconds since the Unix epoch, UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(pub i64);

impl Timestamp {
    /// Current wall-clock time.
    pub fn now() -> Self {
        todo!("SystemTime::now().duration_since(UNIX_EPOCH) as millis")
    }

    /// Construct from raw epoch milliseconds.
    pub const fn from_millis(ms: i64) -> Self {
        Self(ms)
    }

    /// Raw epoch milliseconds.
    pub const fn as_millis(self) -> i64 {
        self.0
    }
}

impl ToSql for Timestamp {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        todo!("ToSqlOutput::from(self.0)")
    }
}

impl FromSql for Timestamp {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        todo!("i64::column_result(value).map(Timestamp)")
    }
}

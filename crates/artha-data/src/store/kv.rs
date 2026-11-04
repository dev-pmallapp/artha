//! Generic namespaced key-value storage (`core_kv`).
//!
//! For small, low-cardinality state that does not deserve its own table and
//! migration: TUI layout prefs, "last seen" markers, a tool's cursor. Anything
//! queried by field, listed in bulk, or growing unbounded belongs in a real
//! namespaced table instead.

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::db::Db;
use crate::Result;

/// Typed JSON values keyed by `(namespace, key)`.
#[derive(Debug, Clone)]
pub struct KvStore {
    db: Db,
    namespace: String,
}

impl KvStore {
    /// Bind to one namespace (convention: the owning crate or tool name).
    pub fn new(db: Db, namespace: impl Into<String>) -> Self {
        Self { db, namespace: namespace.into() }
    }

    /// Get and deserialize.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        todo!()
    }

    /// Upsert.
    pub fn set<T: Serialize + ?Sized>(&self, key: &str, value: &T) -> Result<()> {
        todo!()
    }

    /// Remove; returns whether a row existed.
    pub fn delete(&self, key: &str) -> Result<bool> {
        todo!()
    }

    /// All keys in this namespace, sorted.
    pub fn keys(&self) -> Result<Vec<String>> {
        todo!()
    }
}

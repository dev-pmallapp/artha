//! Repositories over the `core_*` tables that `artha-data` owns.
//!
//! Sibling crates write their own repositories in the same style over their
//! own namespaced tables (see `docs/LLD.md` §5 for the proposed DDL):
//! a small struct holding a [`crate::Db`] clone, with methods that call
//! `db.read` / `db.write`.

mod checkpoint_link;
mod kv;
mod session;

pub use checkpoint_link::{CheckpointLink, CheckpointLinkStore};
pub use kv::KvStore;
pub use session::{
    MessageSeq, NewMessage, NewSession, Role, Session, SessionFilter, SessionId, SessionStore,
    SessionSummary, StoredMessage,
};

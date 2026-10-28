//! Persistent transcription history.
//!
//! Every completed transcription is appended to a JSONL file on disk, so
//! results survive restarts and are never only kept in memory. The module
//! has no UI dependencies and is shared by desktop, CLI and mobile.
//!
//! ```text
//! ~/.local/share/whis/history/entries.jsonl
//! ```

mod store;
mod types;

pub use store::{
    DEFAULT_MAX_ENTRIES, append, clear, count, data_dir, delete, entries_path, history_dir, list,
};
pub use types::{HistoryEntry, HistorySource, new_id, now_ms};

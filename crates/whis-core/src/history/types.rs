//! Types for the persistent transcription history.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Where a history entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HistorySource {
    /// Created by a normal recording.
    #[default]
    Recording,
    /// Created by re-transcribing the last recording.
    Retranscription,
}

/// A single persisted transcription.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Unique, sortable identifier (`<unix-ms-hex>-<counter>`).
    pub id: String,
    /// Creation time in milliseconds since the Unix epoch.
    pub created_at_ms: u64,
    /// Final text (after optional post-processing).
    pub text: String,
    /// Text as returned by the transcription provider, before post-processing.
    #[serde(default)]
    pub raw_text: String,
    /// Whether `text` differs from `raw_text` because post-processing ran.
    #[serde(default)]
    pub post_processed: bool,
    /// Provider identifier, e.g. `local` or `openai`.
    #[serde(default)]
    pub provider: String,
    /// Model identifier or file name, when known.
    #[serde(default)]
    pub model: Option<String>,
    /// Language hint used for transcription.
    #[serde(default)]
    pub language: Option<String>,
    /// Recording duration in seconds.
    #[serde(default)]
    pub duration_secs: f32,
    /// Origin of this entry.
    #[serde(default)]
    pub source: HistorySource,
    /// For re-transcriptions: id of the entry this was derived from.
    #[serde(default)]
    pub parent_id: Option<String>,
}

impl HistoryEntry {
    /// Create an entry with a fresh id and the current timestamp.
    pub fn new(text: impl Into<String>, provider: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            created_at_ms: now_ms(),
            text: text.into(),
            raw_text: String::new(),
            post_processed: false,
            provider: provider.into(),
            model: None,
            language: None,
            duration_secs: 0.0,
            source: HistorySource::Recording,
            parent_id: None,
        }
    }

    /// First line of the text, truncated to `max_chars`.
    ///
    /// Used for list previews without shipping the whole transcript to the UI.
    pub fn preview(&self, max_chars: usize) -> String {
        let first_line = self
            .text
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        let mut preview: String = first_line.chars().take(max_chars).collect();
        if first_line.chars().count() > max_chars {
            preview.push_str("...");
        }
        preview
    }
}

/// Current time in milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Generate a unique, roughly sortable id.
///
/// The timestamp prefix keeps entries sorted when read back, the counter
/// disambiguates entries created within the same millisecond.
pub fn new_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed) & 0xffff;
    format!("{:x}-{:04x}", now_ms(), counter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_uses_first_non_empty_line() {
        let mut entry = HistoryEntry::new("\n\nHello world\nsecond line", "test");
        assert_eq!(entry.preview(20), "Hello world");

        entry.text = "a".repeat(30);
        assert_eq!(entry.preview(10), "aaaaaaaaaa...");
    }

    #[test]
    fn ids_are_unique() {
        let a = new_id();
        let b = new_id();
        assert_ne!(a, b);
    }
}

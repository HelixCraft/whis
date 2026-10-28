//! Append-only JSONL storage for transcription history.
//!
//! One entry per line keeps appends cheap and crash-safe: a torn write can
//! only damage the last line, which is skipped on read. Operations that
//! remove entries rewrite the file into a temporary file and rename it
//! atomically, so a crash never leaves a truncated history.

use super::HistoryEntry;
use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// Default number of entries kept on disk.
pub const DEFAULT_MAX_ENTRIES: usize = crate::settings::DEFAULT_MAX_HISTORY_ENTRIES;

/// File name inside the history directory.
pub const FILE_NAME: &str = "entries.jsonl";

/// Base directory for whis runtime data (`~/.local/share/whis` on Linux).
pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("whis")
}

/// Directory containing the history file.
pub fn history_dir() -> PathBuf {
    data_dir().join("history")
}

/// Path of the JSONL history file.
pub fn entries_path() -> PathBuf {
    history_dir().join(FILE_NAME)
}

/// Load entries, newest first.
///
/// `max` limits the number of returned entries (0 = no limit). Malformed
/// lines are skipped so a single bad write cannot break the whole history.
pub fn list(max: usize) -> Result<Vec<HistoryEntry>> {
    list_at(&entries_path(), max)
}

/// Append one entry and enforce the retention limit.
pub fn append(entry: &HistoryEntry, max_entries: usize) -> Result<()> {
    let path = entries_path();
    append_at(&path, entry)?;
    prune_at(&path, max_entries)
}

/// Delete a single entry. Returns `true` if an entry was removed.
pub fn delete(id: &str) -> Result<bool> {
    delete_at(&entries_path(), id)
}

/// Remove all entries.
pub fn clear() -> Result<()> {
    let path = entries_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create history directory")?;
    }
    write_atomic(&path, &[])
}

/// Number of stored entries.
pub fn count() -> Result<usize> {
    Ok(list(0)?.len())
}

fn list_at(path: &Path, max: usize) -> Result<Vec<HistoryEntry>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).context("Failed to open history file"),
    };

    let mut entries = Vec::new();
    for (line_no, line) in BufReader::new(file).lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                crate::verbose!("History: failed to read line {}: {}", line_no + 1, e);
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<HistoryEntry>(&line) {
            Ok(entry) => entries.push(entry),
            Err(e) => crate::verbose!("History: skipping malformed line {}: {}", line_no + 1, e),
        }
    }

    entries.reverse();
    if max > 0 && entries.len() > max {
        entries.truncate(max);
    }
    Ok(entries)
}

fn append_at(path: &Path, entry: &HistoryEntry) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create history directory")?;
    }

    let mut line = serde_json::to_string(entry).context("Failed to serialize history entry")?;
    line.push('\n');

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .context("Failed to open history file for append")?;
    file.write_all(line.as_bytes())
        .context("Failed to append history entry")?;
    file.sync_data().context("Failed to flush history entry")?;
    Ok(())
}

fn delete_at(path: &Path, id: &str) -> Result<bool> {
    let entries = read_all(path)?;
    let before = entries.len();
    let kept: Vec<HistoryEntry> = entries.into_iter().filter(|e| e.id != id).collect();
    if kept.len() == before {
        return Ok(false);
    }
    write_atomic(path, &kept)?;
    Ok(true)
}

fn prune_at(path: &Path, max_entries: usize) -> Result<()> {
    if max_entries == 0 {
        return Ok(());
    }
    let entries = read_all(path)?;
    if entries.len() <= max_entries {
        return Ok(());
    }
    let keep_from = entries.len() - max_entries;
    let kept: Vec<HistoryEntry> = entries.into_iter().skip(keep_from).collect();
    write_atomic(path, &kept)
}

fn read_all(path: &Path) -> Result<Vec<HistoryEntry>> {
    let mut entries = list_at(path, 0)?;
    entries.reverse(); // oldest first, matching file order
    Ok(entries)
}

fn write_atomic(path: &Path, entries: &[HistoryEntry]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create history directory")?;
    }

    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut file = File::create(&tmp).context("Failed to create temporary history file")?;
        for entry in entries {
            serde_json::to_writer(&mut file, entry).context("Failed to serialize history entry")?;
            file.write_all(b"\n")
                .context("Failed to write history entry")?;
        }
        file.sync_all()
            .context("Failed to flush temporary history file")?;
    }
    fs::rename(&tmp, path).context("Failed to replace history file")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistorySource;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "whis-history-test-{}-{}-{}",
            std::process::id(),
            name,
            super::super::new_id()
        ))
    }

    fn entry(text: &str) -> HistoryEntry {
        let mut e = HistoryEntry::new(text, "test");
        e.duration_secs = 1.5;
        e.source = HistorySource::Recording;
        e
    }

    #[test]
    fn append_list_delete_clear() {
        let path = temp_path("basic");
        let a = entry("first");
        let b = entry("second");
        append_at(&path, &a).unwrap();
        append_at(&path, &b).unwrap();

        let listed = list_at(&path, 0).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].text, "second");

        assert!(delete_at(&path, &a.id).unwrap());
        assert!(!delete_at(&path, &a.id).unwrap());
        assert_eq!(list_at(&path, 0).unwrap().len(), 1);

        write_atomic(&path, &[]).unwrap();
        assert!(list_at(&path, 0).unwrap().is_empty());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn prune_keeps_newest() {
        let path = temp_path("prune");
        for i in 0..5 {
            append_at(&path, &entry(&format!("entry-{i}"))).unwrap();
        }
        prune_at(&path, 3).unwrap();
        let listed = list_at(&path, 0).unwrap();
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].text, "entry-4");
        assert_eq!(listed[2].text, "entry-2");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let path = temp_path("malformed");
        append_at(&path, &entry("good")).unwrap();
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"{not json}\n").unwrap();
        append_at(&path, &entry("also good")).unwrap();

        let listed = list_at(&path, 0).unwrap();
        assert_eq!(listed.len(), 2);
        let _ = fs::remove_file(path);
    }
}

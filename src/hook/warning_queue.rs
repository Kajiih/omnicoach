//! Per-session queue of feedback to deliver to the model later, for agents whose tool-call hook
//! cannot return text to the model directly.
//!
//! The queue is a JSON-lines file in the session's state directory. Both writers and the drain
//! synchronize under an exclusive advisory lock on the file itself, and the drain truncates the
//! queue in place without temporary files or inode renames.

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

const QUEUE_FILENAME: &str = "omnicoach_lint_warnings.jsonl";
const MAX_QUEUED_WARNINGS: usize = 64;

/// Warning queue stored in one session's state directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WarningQueue {
    path: PathBuf,
}

impl WarningQueue {
    #[must_use]
    pub(super) fn in_dir(state_directory: &Path) -> Self {
        Self {
            path: state_directory.join(QUEUE_FILENAME),
        }
    }

    /// Appends warnings, skipping blank ones and capping the queue at [`MAX_QUEUED_WARNINGS`]
    /// unique entries. Best effort: I/O errors drop the warnings, since a hook must never fail
    /// the agent's tool call.
    pub(super) fn push(&self, warnings: &[String]) {
        if warnings.is_empty() {
            return;
        }
        if let Some(parent) = self.path.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return;
        }

        let Ok(mut file) = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&self.path)
        else {
            return;
        };
        if file.lock().is_err() {
            return;
        }

        let mut content = String::new();
        if file.read_to_string(&mut content).is_err() {
            return;
        }
        let queued = collect_unique_warnings(&content, warnings);

        if file.set_len(0).is_err() || file.seek(SeekFrom::Start(0)).is_err() {
            return;
        }
        for warning in &queued {
            if let Ok(json_line) = serde_json::to_string(warning) {
                let _ = writeln!(file, "{json_line}");
            }
        }
    }

    /// Removes and returns the queued warnings in insertion order, without duplicates.
    #[must_use]
    pub(super) fn drain(&self) -> Vec<String> {
        let Ok(mut file) = OpenOptions::new().read(true).write(true).open(&self.path) else {
            return Vec::new();
        };
        if file.lock().is_err() {
            return Vec::new();
        }

        let mut content = String::new();
        if file.read_to_string(&mut content).is_err() {
            return Vec::new();
        }
        let _ = file.set_len(0);

        collect_unique_warnings(&content, &[])
    }
}

fn collect_unique_warnings(existing_jsonl: &str, new_warnings: &[String]) -> Vec<String> {
    let existing = existing_jsonl
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_str::<String>(line).ok());
    let added = new_warnings
        .iter()
        .map(|warning| warning.trim())
        .filter(|warning| !warning.is_empty())
        .map(str::to_owned);
    let mut seen = HashSet::new();
    let mut queued: Vec<String> = existing
        .chain(added)
        .filter(|warning| seen.insert(warning.clone()))
        .collect();
    if queued.len() > MAX_QUEUED_WARNINGS {
        let overflow = queued.len() - MAX_QUEUED_WARNINGS;
        queued.drain(0..overflow);
    }
    queued
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_returns_unique_warnings_in_order_and_empties_the_queue() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let queue = WarningQueue::in_dir(temp_dir.path());

        queue.push(&[
            "warning one\nwith details".to_owned(),
            "warning two".to_owned(),
            "warning one\nwith details".to_owned(),
            "  ".to_owned(),
        ]);

        assert_eq!(
            queue.drain(),
            [
                "warning one\nwith details".to_owned(),
                "warning two".to_owned()
            ]
        );
        assert_eq!(queue.drain(), Vec::<String>::new());
    }

    #[test]
    fn drain_on_missing_queue_returns_empty() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let queue = WarningQueue::in_dir(temp_dir.path());
        assert_eq!(queue.drain(), Vec::<String>::new());
    }

    #[test]
    fn push_after_drain_records_new_warnings() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let queue = WarningQueue::in_dir(temp_dir.path());

        queue.push(&["initial warning".to_owned()]);
        let _ = queue.drain();

        queue.push(&["subsequent warning".to_owned()]);
        assert_eq!(queue.drain(), ["subsequent warning".to_owned()]);
    }

    #[test]
    fn push_caps_queue_at_max_queued_warnings() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let queue = WarningQueue::in_dir(temp_dir.path());

        let first_batch: Vec<String> = (0..40)
            .map(|index| format!("warning number {index}"))
            .collect();
        let second_batch: Vec<String> = (40..70)
            .map(|index| format!("warning number {index}"))
            .collect();
        queue.push(&first_batch);
        queue.push(&second_batch);

        let drained = queue.drain();
        let expected: Vec<String> = (6..70)
            .map(|index| format!("warning number {index}"))
            .collect();
        assert_eq!(drained, expected);
    }
}

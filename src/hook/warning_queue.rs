//! Per-session queue of feedback to deliver to the model later, for agents whose tool-call hook
//! cannot return text to the model directly.
//!
//! The queue is a JSON-lines file in the session's state directory. Both writers and the drain
//! synchronize under an exclusive advisory lock on the file itself, and the drain truncates the
//! queue in place without temporary files or inode renames.

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

const QUEUE_FILENAME: &str = "omnicoach_lint_warnings.jsonl";

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

    /// Appends warnings, skipping blank ones. Best effort: I/O errors drop the warnings, since a
    /// hook must never fail the agent's tool call.
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
            .append(true)
            .open(&self.path)
        else {
            return;
        };
        if file.lock().is_err() {
            return;
        }

        for warning in warnings {
            let trimmed = warning.trim();
            if !trimmed.is_empty()
                && let Ok(json_line) = serde_json::to_string(trimmed)
            {
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

        let mut seen = HashSet::new();
        content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter_map(|line| serde_json::from_str::<String>(line).ok())
            .filter(|warning| seen.insert(warning.clone()))
            .collect()
    }
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
}

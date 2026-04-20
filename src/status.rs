//! `.amos-status` file handling.
//!
//! Status lives outside the plan files so they stay pure specs. The status
//! file is a plain markdown checklist at the scan root; each line names a node
//! with a checkbox state:
//!
//! ```text
//! - [x] @github:tatolab/streamlib#320
//! - [~] @github:tatolab/streamlib#325
//! - [ ] @github:tatolab/streamlib#326
//! ```
//!
//! `[x]` = done, `[~]` = in-progress, `[ ]` (or absence) = pending. Anything
//! not listed in the file is treated as pending.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const STATUS_FILENAME: &str = ".amos-status";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    InProgress,
    Done,
}

impl Status {
    pub fn checkbox(&self) -> char {
        match self {
            Status::Pending => ' ',
            Status::InProgress => '~',
            Status::Done => 'x',
        }
    }
}

/// Parsed `.amos-status` file, keyed by node name.
#[derive(Debug, Default, Clone)]
pub struct StatusFile {
    path: PathBuf,
    entries: HashMap<String, Status>,
}

impl StatusFile {
    /// Load the status file at `<scan_root>/.amos-status`. Missing file returns
    /// an empty StatusFile — not an error.
    pub fn load(scan_root: &Path) -> Result<Self> {
        let path = scan_root.join(STATUS_FILENAME);
        if !path.exists() {
            return Ok(Self {
                path,
                entries: HashMap::new(),
            });
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let mut entries = HashMap::new();
        for line in content.lines() {
            if let Some((status, name)) = parse_line(line) {
                entries.insert(name, status);
            }
        }
        Ok(Self { path, entries })
    }

    pub fn get(&self, name: &str) -> Status {
        self.entries.get(name).copied().unwrap_or(Status::Pending)
    }

    pub fn set(&mut self, name: &str, status: Status) {
        if matches!(status, Status::Pending) {
            self.entries.remove(name);
        } else {
            self.entries.insert(name.to_string(), status);
        }
    }

    pub fn remove(&mut self, name: &str) {
        self.entries.remove(name);
    }

    /// Write the file back, sorting entries by name for a stable diff.
    pub fn save(&self) -> Result<()> {
        let mut lines: Vec<String> = self
            .entries
            .iter()
            .map(|(name, status)| format!("- [{}] {}", status.checkbox(), name))
            .collect();
        lines.sort();

        if lines.is_empty() {
            // Don't leave an empty file on disk; remove instead.
            if self.path.exists() {
                fs::remove_file(&self.path)
                    .with_context(|| format!("removing empty {}", self.path.display()))?;
            }
            return Ok(());
        }

        let content = lines.join("\n") + "\n";
        fs::write(&self.path, content)
            .with_context(|| format!("writing {}", self.path.display()))?;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Parse a single `- [X] name` line. Ignores blank/comment lines.
fn parse_line(line: &str) -> Option<(Status, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let rest = trimmed.strip_prefix("- [")?;
    let (state_char, rest) = rest.split_once(']')?;
    let state = match state_char.trim() {
        "x" | "X" => Status::Done,
        "~" => Status::InProgress,
        "" | " " => Status::Pending,
        _ => return None,
    };
    let name = rest.trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some((state, name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn parse_basic_entries() {
        assert_eq!(
            parse_line("- [x] @github:org/repo#1"),
            Some((Status::Done, "@github:org/repo#1".to_string()))
        );
        assert_eq!(
            parse_line("- [~] in-progress-task"),
            Some((Status::InProgress, "in-progress-task".to_string()))
        );
        assert_eq!(
            parse_line("- [ ] pending-task"),
            Some((Status::Pending, "pending-task".to_string()))
        );
    }

    #[test]
    fn parse_skips_non_entry_lines() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("# header"), None);
        assert_eq!(parse_line("random text"), None);
        assert_eq!(parse_line("- [y] unknown-state"), None);
    }

    #[test]
    fn roundtrip_save_and_load() {
        let dir = tempdir().unwrap();
        let mut sf = StatusFile::load(dir.path()).unwrap();
        assert!(sf.is_empty());

        sf.set("node-a", Status::Done);
        sf.set("node-b", Status::InProgress);
        sf.save().unwrap();

        let reloaded = StatusFile::load(dir.path()).unwrap();
        assert_eq!(reloaded.get("node-a"), Status::Done);
        assert_eq!(reloaded.get("node-b"), Status::InProgress);
        assert_eq!(reloaded.get("node-c"), Status::Pending);
    }

    #[test]
    fn setting_pending_removes_entry() {
        let dir = tempdir().unwrap();
        let mut sf = StatusFile::load(dir.path()).unwrap();
        sf.set("node-a", Status::Done);
        sf.set("node-a", Status::Pending);
        assert_eq!(sf.len(), 0);
    }

    #[test]
    fn save_removes_file_when_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(STATUS_FILENAME);
        // Seed a file
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "- [x] node-a").unwrap();
        drop(f);

        let mut sf = StatusFile::load(dir.path()).unwrap();
        sf.remove("node-a");
        sf.save().unwrap();

        assert!(!path.exists(), "empty status file should be removed");
    }
}

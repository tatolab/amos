//! One-shot migration from the legacy frontmatter format to the typed model.
//!
//! Legacy frontmatter used a single `dependencies:` list with `up:`/`down:`
//! prefixes and an in-frontmatter `status:` field. This module converts it:
//!
//! - `up:X` entries become `blocked_by: [X]`.
//! - `down:X` entries become `blocks: [X]`.
//! - `status: <value>` is stripped from frontmatter and written to
//!   `.amos-status` when the value is `in-progress`/`in_progress` or
//!   `completed`/`done`. `pending` (or absent) is the default and produces no
//!   entry.
//!
//! Grouping ("umbrellas") is intentionally not handled here — that concept is
//! delegated to GitHub milestones, resolved through the adapter in a separate
//! feature. The migration produces a flat DAG of typed-edge relationships.
//!
//! With `dry_run`, no files are written; a [`MigrationReport`] describing what
//! would change is still returned so callers can surface the diff summary.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::status::{Status, StatusFile};

/// Per-file migration result.
#[derive(Debug, Clone)]
pub struct FileReport {
    pub path: PathBuf,
    pub kind: FileChange,
    pub blocked_by_added: usize,
    pub blocks_added: usize,
    pub status_moved: Option<Status>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChange {
    /// The file had legacy fields and was (or would be) rewritten.
    Migrated,
    /// Already in the new format — no changes needed.
    Unchanged,
    /// Not an amos block; skipped.
    NotAmos,
}

#[derive(Debug, Default)]
pub struct MigrationReport {
    pub files: Vec<FileReport>,
    pub status_file_entries: usize,
}

impl MigrationReport {
    pub fn migrated_files(&self) -> impl Iterator<Item = &FileReport> {
        self.files.iter().filter(|r| r.kind == FileChange::Migrated)
    }

    pub fn summary(&self) -> String {
        let migrated: Vec<&FileReport> = self.migrated_files().collect();
        let total_edges: usize = migrated
            .iter()
            .map(|r| r.blocked_by_added + r.blocks_added)
            .sum();
        format!(
            "{} files migrated, {} edges converted, {} status entries",
            migrated.len(),
            total_edges,
            self.status_file_entries
        )
    }
}

/// Migrate every markdown file under `scan_root`. Status entries accumulate
/// into a StatusFile that is written (or skipped, on dry_run) at the end.
pub fn migrate_tree(scan_root: &Path, dry_run: bool) -> Result<MigrationReport> {
    let mut report = MigrationReport::default();
    let mut status_file = StatusFile::load(scan_root)?;
    let initial_status_len = status_file.len();

    // Walk all .md files under scan_root.
    let walker = ignore::WalkBuilder::new(scan_root)
        .standard_filters(true)
        .build();

    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let file_report = migrate_file(path, &mut status_file, dry_run)?;
        report.files.push(file_report);
    }

    if !dry_run {
        status_file.save()?;
    }
    report.status_file_entries = status_file.len().saturating_sub(initial_status_len);

    Ok(report)
}

/// Migrate a single file. On dry_run, computes the change without writing.
pub fn migrate_file(
    path: &Path,
    status_file: &mut StatusFile,
    dry_run: bool,
) -> Result<FileReport> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;

    let Some((fm_str, body)) = split_frontmatter(&content) else {
        return Ok(FileReport {
            path: path.to_path_buf(),
            kind: FileChange::NotAmos,
            blocked_by_added: 0,
            blocks_added: 0,
            status_moved: None,
        });
    };

    let mut fm: serde_yaml::Value = serde_yaml::from_str(fm_str)
        .with_context(|| format!("parsing frontmatter YAML in {}", path.display()))?;

    // Only operate on amos blocks.
    if fm.get("whoami").and_then(|v| v.as_str()) != Some("amos") {
        return Ok(FileReport {
            path: path.to_path_buf(),
            kind: FileChange::NotAmos,
            blocked_by_added: 0,
            blocks_added: 0,
            status_moved: None,
        });
    }

    // Collect legacy dependencies.
    let deps = fm
        .get("dependencies")
        .and_then(|v| v.as_sequence())
        .cloned()
        .unwrap_or_default();

    let mut new_blocked_by: Vec<String> = Vec::new();
    let mut new_blocks: Vec<String> = Vec::new();

    for dep in &deps {
        let Some(s) = dep.as_str() else { continue };
        if let Some(target) = s.strip_prefix("up:") {
            new_blocked_by.push(target.trim().to_string());
        } else if let Some(target) = s.strip_prefix("down:") {
            new_blocks.push(target.trim().to_string());
        }
    }

    // Status field.
    let old_status_raw = fm.get("status").and_then(|v| v.as_str()).map(String::from);
    let status_moved = match old_status_raw.as_deref() {
        Some("done") | Some("completed") => Some(Status::Done),
        Some("in-progress") | Some("in_progress") => Some(Status::InProgress),
        _ => None, // "pending", empty, or absent — default
    };

    // Nothing to migrate if there are no legacy fields.
    let has_legacy_fields = !deps.is_empty() || old_status_raw.is_some();
    if !has_legacy_fields {
        return Ok(FileReport {
            path: path.to_path_buf(),
            kind: FileChange::Unchanged,
            blocked_by_added: 0,
            blocks_added: 0,
            status_moved: None,
        });
    }

    let name = fm
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let blocked_by_added = new_blocked_by.len();
    let blocks_added = new_blocks.len();

    if let Some(fm_map) = fm.as_mapping_mut() {
        fm_map.remove("dependencies");
        fm_map.remove("status");

        if !new_blocked_by.is_empty() {
            merge_into_list(fm_map, "blocked_by", &new_blocked_by);
        }
        if !new_blocks.is_empty() {
            merge_into_list(fm_map, "blocks", &new_blocks);
        }
    }

    if !dry_run {
        let new_fm_str = serde_yaml::to_string(&fm)
            .with_context(|| format!("serializing migrated frontmatter for {}", path.display()))?;
        let new_content = format!("---\n{}---\n{}", new_fm_str, body);
        fs::write(path, new_content)
            .with_context(|| format!("writing migrated {}", path.display()))?;
    }

    if let Some(status) = status_moved {
        if !name.is_empty() {
            status_file.set(&name, status);
        }
    }

    Ok(FileReport {
        path: path.to_path_buf(),
        kind: FileChange::Migrated,
        blocked_by_added,
        blocks_added,
        status_moved,
    })
}

fn merge_into_list(
    map: &mut serde_yaml::Mapping,
    key: &str,
    new_values: &[String],
) {
    let existing = map
        .get(key)
        .and_then(|v| v.as_sequence())
        .cloned()
        .unwrap_or_default();
    let mut combined: Vec<serde_yaml::Value> = existing;
    for v in new_values {
        combined.push(serde_yaml::Value::String(v.clone()));
    }
    map.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::Sequence(combined),
    );
}

/// Split a markdown file into (frontmatter_body, rest_of_file). Returns None
/// if there's no leading frontmatter block.
fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    // Allow leading blank lines before the opening ---.
    let trimmed_start = content.trim_start_matches(|c: char| c == '\n' || c == '\r');

    if !trimmed_start.starts_with("---\n") && !trimmed_start.starts_with("---\r\n") {
        return None;
    }
    let after_open = trimmed_start.strip_prefix("---\n").or_else(|| trimmed_start.strip_prefix("---\r\n"))?;

    // Find closing --- on its own line.
    let mut offset = 0;
    for line in after_open.split_inclusive('\n') {
        let line_trim = line.trim_end_matches(['\n', '\r']);
        if line_trim == "---" {
            let fm = &after_open[..offset];
            let rest = &after_open[offset + line.len()..];
            return Some((fm, rest));
        }
        offset += line.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_md(dir: &Path, name: &str, content: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn migrate_non_umbrella_converts_down_to_blocks() {
        let dir = tempdir().unwrap();
        let path = write_md(
            dir.path(),
            "a.md",
            r#"---
whoami: amos
name: task-a
description: simple task
dependencies:
  - up:upstream-task
  - down:downstream-task
---

body content
"#,
        );

        let mut sf = StatusFile::load(dir.path()).unwrap();
        let report = migrate_file(&path, &mut sf, false).unwrap();
        assert_eq!(report.kind, FileChange::Migrated);
        assert_eq!(report.blocked_by_added, 1);
        assert_eq!(report.blocks_added, 1);

        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("blocked_by:"));
        assert!(after.contains("blocks:"));
        assert!(!after.contains("dependencies:"));
        assert!(!after.contains("up:"));
        assert!(!after.contains("down:"));
        assert!(after.contains("body content")); // body preserved
    }

    #[test]
    fn migrate_status_in_frontmatter_moves_to_status_file() {
        let dir = tempdir().unwrap();
        let path = write_md(
            dir.path(),
            "a.md",
            r#"---
whoami: amos
name: task-a
status: in-progress
description: "task in flight"
---

body
"#,
        );

        let mut sf = StatusFile::load(dir.path()).unwrap();
        let report = migrate_file(&path, &mut sf, false).unwrap();
        assert_eq!(report.kind, FileChange::Migrated);
        assert_eq!(report.status_moved, Some(Status::InProgress));

        // Frontmatter no longer has status.
        let after = fs::read_to_string(&path).unwrap();
        assert!(!after.contains("status:"));
        assert_eq!(sf.get("task-a"), Status::InProgress);
    }

    #[test]
    fn migrate_pending_status_drops_without_status_entry() {
        let dir = tempdir().unwrap();
        let path = write_md(
            dir.path(),
            "a.md",
            r#"---
whoami: amos
name: task-a
status: pending
---

body
"#,
        );

        let mut sf = StatusFile::load(dir.path()).unwrap();
        let report = migrate_file(&path, &mut sf, false).unwrap();
        assert_eq!(report.kind, FileChange::Migrated);
        assert_eq!(report.status_moved, None);
        assert_eq!(sf.len(), 0);
    }

    #[test]
    fn migrate_already_migrated_file_is_unchanged() {
        let dir = tempdir().unwrap();
        let path = write_md(
            dir.path(),
            "a.md",
            r#"---
whoami: amos
name: task-a
description: "modern format"
blocks:
  - task-b
---
"#,
        );

        let mut sf = StatusFile::load(dir.path()).unwrap();
        let report = migrate_file(&path, &mut sf, false).unwrap();
        assert_eq!(report.kind, FileChange::Unchanged);
    }

    #[test]
    fn migrate_skips_non_amos_files() {
        let dir = tempdir().unwrap();
        let path = write_md(
            dir.path(),
            "readme.md",
            r#"---
title: regular markdown
---

not an amos file
"#,
        );

        let mut sf = StatusFile::load(dir.path()).unwrap();
        let report = migrate_file(&path, &mut sf, false).unwrap();
        assert_eq!(report.kind, FileChange::NotAmos);
    }

    #[test]
    fn migrate_non_umbrella_first_asserts_no_umbrella_fields() {
        // After the umbrella concept was removed, every `down:` entry becomes
        // `blocks:` regardless of description content. This test confirms a
        // description that used to trigger the heuristic is no longer treated
        // specially.
        let dir = tempdir().unwrap();
        let path = write_md(
            dir.path(),
            "u.md",
            r#"---
whoami: amos
name: tracking-node
description: "Umbrella — overarching plan"
dependencies:
  - down:child-a
  - down:child-b
---
"#,
        );

        let mut sf = StatusFile::load(dir.path()).unwrap();
        let report = migrate_file(&path, &mut sf, false).unwrap();
        assert_eq!(report.kind, FileChange::Migrated);
        assert_eq!(report.blocks_added, 2);
        assert_eq!(report.blocked_by_added, 0);

        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("blocks:"));
        assert!(!after.contains("children:"));
    }

    #[test]
    fn migrate_dry_run_leaves_file_untouched() {
        let dir = tempdir().unwrap();
        let original = r#"---
whoami: amos
name: task-a
dependencies:
  - up:other
---
"#;
        let path = write_md(dir.path(), "a.md", original);

        let mut sf = StatusFile::load(dir.path()).unwrap();
        let report = migrate_file(&path, &mut sf, true).unwrap();
        assert_eq!(report.kind, FileChange::Migrated);

        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(after, original, "dry-run must not modify the file");
    }
}

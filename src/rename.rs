//! `amos rename` — rewrite a node's `name:` and every reference to it.
//!
//! Stale references (one file's `blocked_by:` points at a name that was
//! renamed in another file) silently break DAG edges — the edge string
//! doesn't match anything, and the rendered graph just drops the
//! relationship. This command does a coordinated rename across the whole
//! scan tree.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Per-file rename result.
#[derive(Debug, Clone)]
pub struct FileRenameReport {
    pub path: PathBuf,
    /// True if this file was the source of the renamed node (had `name: <old>`).
    pub name_updated: bool,
    /// How many reference lines were rewritten (frontmatter values + body
    /// `@scheme:` refs combined).
    pub refs_updated: usize,
}

#[derive(Debug, Default)]
pub struct RenameReport {
    pub files: Vec<FileRenameReport>,
}

impl RenameReport {
    pub fn total_refs_updated(&self) -> usize {
        self.files.iter().map(|r| r.refs_updated).sum()
    }

    pub fn files_changed(&self) -> usize {
        self.files
            .iter()
            .filter(|r| r.name_updated || r.refs_updated > 0)
            .count()
    }

    pub fn summary(&self) -> String {
        format!(
            "{} files changed, {} references updated",
            self.files_changed(),
            self.total_refs_updated()
        )
    }
}

/// Rename `old_name` to `new_name` across every `.md` file under `scan_root`.
///
/// Rewrites:
/// - `name: <old_name>` → `name: <new_name>` (on whatever file defines it)
/// - Every `blocked_by:`, `blocks:`, `related_to:`, `duplicates:`,
///   `superseded_by:` list entry or scalar value that equals `old_name`.
/// - Every body reference whose string equals `old_name`.
///
/// With `dry_run` set, no files are written; the report still describes what
/// would change.
pub fn rename_tree(
    scan_root: &Path,
    old_name: &str,
    new_name: &str,
    dry_run: bool,
) -> Result<RenameReport> {
    let mut report = RenameReport::default();

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
        let file_report = rename_in_file(path, old_name, new_name, dry_run)?;
        report.files.push(file_report);
    }

    Ok(report)
}

fn rename_in_file(
    path: &Path,
    old_name: &str,
    new_name: &str,
    dry_run: bool,
) -> Result<FileRenameReport> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;

    let mut new_content = String::with_capacity(content.len());
    let mut name_updated = false;
    let mut refs_updated = 0usize;

    for line in content.split_inclusive('\n') {
        let rewritten = rewrite_line(line, old_name, new_name, &mut name_updated, &mut refs_updated);
        new_content.push_str(&rewritten);
    }

    if (name_updated || refs_updated > 0) && !dry_run {
        fs::write(path, &new_content)
            .with_context(|| format!("writing renamed {}", path.display()))?;
    }

    Ok(FileRenameReport {
        path: path.to_path_buf(),
        name_updated,
        refs_updated,
    })
}

/// Rewrite a single line. Looks for:
/// - `name:` frontmatter line whose value matches `old_name`.
/// - Any quoted or bare occurrence of `old_name` as a whole YAML scalar.
/// Avoids partial-string matches (a substring inside a longer name).
fn rewrite_line(
    line: &str,
    old_name: &str,
    new_name: &str,
    name_updated: &mut bool,
    refs_updated: &mut usize,
) -> String {
    // Cheap early-out if the name isn't anywhere in the line.
    if !line.contains(old_name) {
        return line.to_string();
    }

    // Split off trailing newline(s) so we can work on the textual part.
    let (text, trailing_newlines) = split_trailing_newlines(line);

    // Find all occurrences of old_name as a "whole token" — preceded and
    // followed by characters that can't be part of the identifier. YAML
    // quoting (`"`, `'`), whitespace, comma, bracket, end-of-string all count.
    let mut rewritten = String::with_capacity(text.len());
    let mut cursor = 0;
    let bytes = text.as_bytes();
    let old_bytes = old_name.as_bytes();

    while cursor < text.len() {
        if let Some(rel) = text[cursor..].find(old_name) {
            let abs = cursor + rel;
            let before_ok = abs == 0
                || is_boundary_byte(bytes[abs - 1]);
            let after_end = abs + old_bytes.len();
            let after_ok = after_end >= bytes.len()
                || is_boundary_byte(bytes[after_end]);

            // Copy the segment up to the match.
            rewritten.push_str(&text[cursor..abs]);

            if before_ok && after_ok {
                rewritten.push_str(new_name);
                // Heuristic: if this line starts with `name:` and the match
                // lands after the colon, this is the defining name.
                let trimmed_prefix = text[..abs].trim_start();
                if trimmed_prefix.starts_with("name:") {
                    *name_updated = true;
                } else {
                    *refs_updated += 1;
                }
            } else {
                // Not a word boundary — leave as-is.
                rewritten.push_str(&text[abs..abs + old_bytes.len()]);
            }
            cursor = abs + old_bytes.len();
        } else {
            rewritten.push_str(&text[cursor..]);
            break;
        }
    }

    rewritten.push_str(trailing_newlines);
    rewritten
}

fn is_boundary_byte(b: u8) -> bool {
    matches!(
        b,
        b'"' | b'\'' | b' ' | b'\t' | b',' | b'[' | b']' | b'(' | b')' | b'\n' | b'\r'
    )
}

fn split_trailing_newlines(line: &str) -> (&str, &str) {
    let trimmed = line.trim_end_matches(['\n', '\r']);
    let trailing_len = line.len() - trimmed.len();
    (trimmed, &line[line.len() - trailing_len..])
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
    fn rename_updates_name_field_and_refs() {
        let dir = tempdir().unwrap();
        let a = write_md(
            dir.path(),
            "a.md",
            r#"---
whoami: amos
name: "@github:org/repo#1"
blocks:
  - "@github:org/repo#2"
---
"#,
        );
        let b = write_md(
            dir.path(),
            "b.md",
            r#"---
whoami: amos
name: "@github:org/repo#2"
blocked_by:
  - "@github:org/repo#1"
---
"#,
        );

        let report = rename_tree(
            dir.path(),
            "@github:org/repo#1",
            "@github:org/repo#999",
            false,
        )
        .unwrap();
        assert_eq!(report.files_changed(), 2);

        let a_after = fs::read_to_string(&a).unwrap();
        assert!(a_after.contains("@github:org/repo#999"));
        assert!(!a_after.contains("@github:org/repo#1\""));

        let b_after = fs::read_to_string(&b).unwrap();
        assert!(b_after.contains("blocked_by"));
        assert!(b_after.contains("@github:org/repo#999"));
    }

    #[test]
    fn rename_dry_run_leaves_files_untouched() {
        let dir = tempdir().unwrap();
        let original = r#"---
whoami: amos
name: "task-a"
---
"#;
        let path = write_md(dir.path(), "x.md", original);

        let report =
            rename_tree(dir.path(), "task-a", "task-b", true).unwrap();
        assert_eq!(report.files_changed(), 1);

        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(after, original);
    }

    #[test]
    fn rename_does_not_match_partial_substrings() {
        let dir = tempdir().unwrap();
        let path = write_md(
            dir.path(),
            "a.md",
            r#"---
whoami: amos
name: task-a-long
---
"#,
        );

        let report = rename_tree(dir.path(), "task-a", "renamed", false).unwrap();
        assert_eq!(report.files_changed(), 0);

        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("task-a-long"));
        assert!(!after.contains("renamed"));
    }
}

//! `.amosrc.toml` read/write.
//!
//! Small config file stored at the scan root. Today it holds only two things:
//!
//! - `[adapters]` tables (existing) — external adapter command registrations.
//! - `focus` (new) — the milestone the user is currently working on. Used by
//!   `amos next`, `amos blocked`, `amos orphans`, and future milestone-aware
//!   queries to scope their results.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub const AMOSRC_FILENAME: &str = ".amosrc.toml";

pub fn path(scan_root: &Path) -> PathBuf {
    scan_root.join(AMOSRC_FILENAME)
}

/// Read the focused milestone if set.
pub fn read_focus(scan_root: &Path) -> Result<Option<String>> {
    let p = path(scan_root);
    if !p.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&p)
        .with_context(|| format!("reading {}", p.display()))?;
    let table: toml::Table = content
        .parse()
        .with_context(|| format!("parsing {}", p.display()))?;
    Ok(table.get("focus").and_then(|v| v.as_str()).map(String::from))
}

/// Set (or clear) the focused milestone, preserving other fields in the file.
pub fn write_focus(scan_root: &Path, focus: Option<&str>) -> Result<()> {
    let p = path(scan_root);
    let mut table: toml::Table = if p.exists() {
        fs::read_to_string(&p)
            .with_context(|| format!("reading {}", p.display()))?
            .parse()
            .with_context(|| format!("parsing {}", p.display()))?
    } else {
        toml::Table::new()
    };

    match focus {
        Some(value) => {
            table.insert("focus".to_string(), toml::Value::String(value.to_string()));
        }
        None => {
            table.remove("focus");
        }
    }

    if table.is_empty() {
        if p.exists() {
            fs::remove_file(&p)
                .with_context(|| format!("removing empty {}", p.display()))?;
        }
    } else {
        let serialized = toml::to_string(&table)
            .context("serializing .amosrc.toml")?;
        fs::write(&p, serialized)
            .with_context(|| format!("writing {}", p.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn roundtrip_focus_value() {
        let dir = tempdir().unwrap();
        assert_eq!(read_focus(dir.path()).unwrap(), None);

        write_focus(dir.path(), Some("GPU Capability Rewrite")).unwrap();
        assert_eq!(
            read_focus(dir.path()).unwrap(),
            Some("GPU Capability Rewrite".to_string())
        );

        write_focus(dir.path(), None).unwrap();
        assert_eq!(read_focus(dir.path()).unwrap(), None);
    }

    #[test]
    fn preserves_other_fields() {
        let dir = tempdir().unwrap();
        let p = path(dir.path());
        fs::write(
            &p,
            r#"
[adapters.jira]
command = "npx @openclaw/amos-adapter-jira"
"#,
        )
        .unwrap();

        write_focus(dir.path(), Some("Some Milestone")).unwrap();

        let after = fs::read_to_string(&p).unwrap();
        assert!(after.contains("focus = \"Some Milestone\""));
        assert!(after.contains("[adapters.jira]"));
        assert!(after.contains("npx @openclaw/amos-adapter-jira"));
    }
}

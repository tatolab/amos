use std::collections::HashMap;
use std::path::Path;

const STATUS_FILE: &str = ".amos-status";

/// Read the status file from the scan root.
/// Returns a map of node name → status string.
pub fn read_status_file(scan_root: &Path) -> HashMap<String, String> {
    let path = scan_root.join(STATUS_FILE);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };

    let mut statuses = HashMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some((name, status)) = parse_status_line(trimmed) {
            statuses.insert(name, status);
        }
    }
    statuses
}

/// Write or update a single node's status in the status file.
pub fn write_status(scan_root: &Path, name: &str, status: &str) -> std::io::Result<()> {
    let path = scan_root.join(STATUS_FILE);
    let mut entries = read_entries(&path);

    // Canonical aliases for compact display
    let symbol = match status {
        "done" => "x",
        "in-progress" => "~",
        other => other,
    };

    // Update existing or append
    let mut found = false;
    for entry in &mut entries {
        if entry.name == name {
            entry.symbol = symbol.to_string();
            found = true;
            break;
        }
    }
    if !found {
        entries.push(StatusEntry {
            symbol: symbol.to_string(),
            name: name.to_string(),
        });
    }

    write_entries(&path, &entries)
}

/// Remove a node's status from the status file.
pub fn clear_status(scan_root: &Path, name: &str) -> std::io::Result<()> {
    let path = scan_root.join(STATUS_FILE);
    let mut entries = read_entries(&path);
    entries.retain(|e| e.name != name);
    write_entries(&path, &entries)
}

// Internal types and helpers

struct StatusEntry {
    symbol: String,
    name: String,
}

fn parse_status_line(line: &str) -> Option<(String, String)> {
    // - [x] node-name  → "done"
    // - [~] node-name  → "in-progress"
    // - [ ] node-name  → skip (not started, no status)
    // - [closed] node  → "closed"
    // - [In Review] n  → "In Review"
    let rest = line.strip_prefix("- [")?;
    let (symbol, rest) = rest.split_once(']')?;
    let name = rest.trim_start_matches(' ').trim();

    if name.is_empty() {
        return None;
    }

    let status = match symbol.trim() {
        "x" => "done",
        "~" => "in-progress",
        "" | " " => return None, // not started
        other => other,
    };

    Some((name.to_string(), status.to_string()))
}

fn read_entries(path: &Path) -> Vec<StatusEntry> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- [") {
            if let Some((symbol, rest)) = rest.split_once(']') {
                let name = rest.trim_start_matches(' ').trim();
                if !name.is_empty() {
                    entries.push(StatusEntry {
                        symbol: symbol.trim().to_string(),
                        name: name.to_string(),
                    });
                }
            }
        }
    }
    entries
}

fn write_entries(path: &Path, entries: &[StatusEntry]) -> std::io::Result<()> {
    let mut content = String::new();
    for entry in entries {
        content.push_str(&format!("- [{}] {}\n", entry.symbol, entry.name));
    }
    std::fs::write(path, content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_read_empty() {
        let dir = setup();
        let statuses = read_status_file(dir.path());
        assert!(statuses.is_empty());
    }

    #[test]
    fn test_write_and_read() {
        let dir = setup();
        write_status(dir.path(), "task-a", "done").unwrap();
        write_status(dir.path(), "task-b", "in-progress").unwrap();

        let statuses = read_status_file(dir.path());
        assert_eq!(statuses.get("task-a").map(|s| s.as_str()), Some("done"));
        assert_eq!(statuses.get("task-b").map(|s| s.as_str()), Some("in-progress"));
    }

    #[test]
    fn test_update_existing() {
        let dir = setup();
        write_status(dir.path(), "task-a", "in-progress").unwrap();
        write_status(dir.path(), "task-a", "done").unwrap();

        let statuses = read_status_file(dir.path());
        assert_eq!(statuses.get("task-a").map(|s| s.as_str()), Some("done"));

        // Verify file has only one entry
        let content = fs::read_to_string(dir.path().join(".amos-status")).unwrap();
        assert_eq!(content.matches("task-a").count(), 1);
    }

    #[test]
    fn test_clear_status() {
        let dir = setup();
        write_status(dir.path(), "task-a", "done").unwrap();
        write_status(dir.path(), "task-b", "done").unwrap();
        clear_status(dir.path(), "task-a").unwrap();

        let statuses = read_status_file(dir.path());
        assert!(!statuses.contains_key("task-a"));
        assert_eq!(statuses.get("task-b").map(|s| s.as_str()), Some("done"));
    }

    #[test]
    fn test_parse_all_symbols() {
        let dir = setup();
        let content = "- [x] done-task\n- [~] wip-task\n- [ ] pending-task\n";
        fs::write(dir.path().join(".amos-status"), content).unwrap();

        let statuses = read_status_file(dir.path());
        assert_eq!(statuses.get("done-task").map(|s| s.as_str()), Some("done"));
        assert_eq!(statuses.get("wip-task").map(|s| s.as_str()), Some("in-progress"));
        assert!(!statuses.contains_key("pending-task")); // [ ] = no status
    }

    #[test]
    fn test_arbitrary_status_strings() {
        let dir = setup();
        write_status(dir.path(), "task-a", "closed").unwrap();
        write_status(dir.path(), "task-b", "In Review").unwrap();

        let statuses = read_status_file(dir.path());
        assert_eq!(statuses.get("task-a").map(|s| s.as_str()), Some("closed"));
        assert_eq!(statuses.get("task-b").map(|s| s.as_str()), Some("In Review"));

        // Verify raw file content uses the strings directly
        let content = fs::read_to_string(dir.path().join(".amos-status")).unwrap();
        assert!(content.contains("- [closed] task-a"));
        assert!(content.contains("- [In Review] task-b"));
    }
}

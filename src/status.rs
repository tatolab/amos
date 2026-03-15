use std::collections::HashMap;
use std::path::Path;

/// Status values for a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualStatus {
    InProgress,
    Done,
}

const STATUS_FILE: &str = ".amos-status";

/// Read the status file from the scan root.
/// Returns a map of node name → status.
pub fn read_status_file(scan_root: &Path) -> HashMap<String, ManualStatus> {
    let path = scan_root.join(STATUS_FILE);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };

    let mut statuses = HashMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(name) = parse_status_line(trimmed) {
            statuses.insert(name.0, name.1);
        }
    }
    statuses
}

/// Write or update a single node's status in the status file.
pub fn write_status(scan_root: &Path, name: &str, status: ManualStatus) -> std::io::Result<()> {
    let path = scan_root.join(STATUS_FILE);
    let mut entries = read_entries(&path);

    let symbol = match status {
        ManualStatus::Done => "x",
        ManualStatus::InProgress => "~",
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

fn parse_status_line(line: &str) -> Option<(String, ManualStatus)> {
    // - [x] node-name  → Done
    // - [~] node-name  → InProgress
    // - [ ] node-name  → skip (not started, no status)
    let rest = line.strip_prefix("- [")?;
    let (symbol, rest) = rest.split_once(']')?;
    let name = rest.trim_start_matches(' ').trim();

    if name.is_empty() {
        return None;
    }

    match symbol.trim() {
        "x" => Some((name.to_string(), ManualStatus::Done)),
        "~" => Some((name.to_string(), ManualStatus::InProgress)),
        _ => None, // " " or anything else = not started
    }
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
        write_status(dir.path(), "task-a", ManualStatus::Done).unwrap();
        write_status(dir.path(), "task-b", ManualStatus::InProgress).unwrap();

        let statuses = read_status_file(dir.path());
        assert_eq!(statuses.get("task-a"), Some(&ManualStatus::Done));
        assert_eq!(statuses.get("task-b"), Some(&ManualStatus::InProgress));
    }

    #[test]
    fn test_update_existing() {
        let dir = setup();
        write_status(dir.path(), "task-a", ManualStatus::InProgress).unwrap();
        write_status(dir.path(), "task-a", ManualStatus::Done).unwrap();

        let statuses = read_status_file(dir.path());
        assert_eq!(statuses.get("task-a"), Some(&ManualStatus::Done));

        // Verify file has only one entry
        let content = fs::read_to_string(dir.path().join(".amos-status")).unwrap();
        assert_eq!(content.matches("task-a").count(), 1);
    }

    #[test]
    fn test_clear_status() {
        let dir = setup();
        write_status(dir.path(), "task-a", ManualStatus::Done).unwrap();
        write_status(dir.path(), "task-b", ManualStatus::Done).unwrap();
        clear_status(dir.path(), "task-a").unwrap();

        let statuses = read_status_file(dir.path());
        assert!(!statuses.contains_key("task-a"));
        assert_eq!(statuses.get("task-b"), Some(&ManualStatus::Done));
    }

    #[test]
    fn test_parse_all_symbols() {
        let dir = setup();
        let content = "- [x] done-task\n- [~] wip-task\n- [ ] pending-task\n";
        fs::write(dir.path().join(".amos-status"), content).unwrap();

        let statuses = read_status_file(dir.path());
        assert_eq!(statuses.get("done-task"), Some(&ManualStatus::Done));
        assert_eq!(statuses.get("wip-task"), Some(&ManualStatus::InProgress));
        assert!(!statuses.contains_key("pending-task")); // [ ] = no status
    }
}

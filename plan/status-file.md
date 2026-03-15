---
whoami: amos
name: gh:4
description: Decouple status from frontmatter — store state in .amos-status file
dependencies:
  - down:gh:8
  - down:gh:9
---

Add `src/status.rs` that reads/writes `.amos-status` at the scan root.

File format:
```
- [x] strip-tags
- [~] claude-handoff
- [ ] something-not-started
```

`[x]` = done, `[~]` = in-progress, absent or `[ ]` = not started.

The status module provides:
- `read_status_file(scan_root) -> HashMap<String, ManualStatus>`
- `write_status(scan_root, name, status)` — updates a single entry
- `clear_status(scan_root, name)` — removes an entry

DAG builder reads nodes from scanner, then overlays status from the status file instead of from frontmatter.

---
whoami: amos
name: "gh:9"
description: Remove status field from frontmatter parsing — status lives in .amos-status only
dependencies:
  - "up:gh:4"
  - "down:gh:12"
---

Strip `status` from `RawFrontmatter` and `Node` in `parser.rs`. Node files are immutable specs — you don't edit them to track progress.

`ManualStatus` enum moves to `status.rs` since that's where it's used now.

Update DAG builder to accept status overlay from the status file rather than reading it off each node.

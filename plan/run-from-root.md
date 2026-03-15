---
whoami: amos
name: "@github:tatolab/amos#7"
description: Make amos runnable from the project root with no arguments
dependencies:
  - "up:@github:tatolab/amos#2"
  - "down:@github:tatolab/amos#11"
---

Right now `amos` requires a prompt argument. The common case is: you're in a project directory with amos files, you want to see what's ready and hand the whole thing off to claude.

Changes to `cli.rs` and `main.rs`:
- Make `PROMPT` optional — if absent, default to something like "summarize the work stream and identify what's ready"
- Scan cwd by default (already works)
- Print the DAG summary to stderr so you can see what amos found before claude runs

Done: prompt arg removed, cwd scan is the default.

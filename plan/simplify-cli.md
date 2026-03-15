---
whoami: amos
name: "gh:6"
description: Strip CLI to just amos [--dir path] — no prompt arg
dependencies:
  - "up:gh:3"
  - "down:gh:10"
---

Remove the prompt argument entirely. Amos is not a conversational tool — it scans and dumps.

`cli.rs`: just `--dir` (optional, defaults to cwd).
`main.rs`: scan → parse → build → print. Exit 0 if blocks found, exit 1 if none.

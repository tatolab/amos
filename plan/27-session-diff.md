---
whoami: amos
name: "@github:tatolab/amos#27"
description: Multi-session awareness — detecting what changed between runs
adapters:
  github: builtin
---

@github:tatolab/amos#27

Write a `.amos-snapshot` file after each run with the current state: node name → status. On next run, diff against the snapshot and print changes to stderr before the main output. Keep the snapshot format simple — same checkbox format as `.amos-status`. Don't make this a subcommand, just do it automatically.

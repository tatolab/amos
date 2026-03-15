---
whoami: amos
name: "@github:tatolab/amos#8"
description: CLI subcommands to mutate status — amos done, amos start, amos reset
dependencies:
  - "up:@github:tatolab/amos#4"
  - "down:@github:tatolab/amos#12"
---

Add subcommands to `cli.rs`:
```
amos                        # dump DAG (default, read-only)
amos done <node>            # mark node done
amos start <node>           # mark node in-progress
amos reset <node>           # clear node status
```

Each writes to `.amos-status` via the status module. Prints updated status line to stderr for confirmation.

Use clap subcommands with a default (no subcommand = dump DAG).

---
whoami: amos
name: status-cli
description: CLI subcommands to mutate status — amos done, amos start, amos reset
dependencies:
  - up:status-file
  - down:adapter-framework
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

---
name: amos-prune
description: >
  Remove completed amos nodes not needed by active work.
disable-model-invocation: true
allowed-tools: Bash(amos:*)
---

Run `amos prune` to delete done nodes whose `.md` files and
`.amos-status` entries aren't transitively upstream of any
ready or in-progress node.

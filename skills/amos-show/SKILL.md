---
name: amos-show
description: >
  Resolve a single amos node fully by name. Use when you need
  the complete resolved content of a specific whoami: amos node.
allowed-tools: Bash(amos:*)
---

Run `amos show <name>` to get a single node with all references
resolved and content fully expanded. Pass the node's `name` field
value (e.g. `amos show "@github:tatolab/streamlib#143"`).

---
whoami: amos
name: "gh:15"
description: "Built-in file adapter for @file: references"
dependencies:
  - "up:gh:14"
---

@gh:tatolab/amos#15

Built-in adapter registered by default (no `.amosrc.toml` needed).

For text files: returns content inline.
For binary files (images, PDFs, etc.): returns the file path so Claude Code can read them with its multimodal support.

---
whoami: amos
name: "@github:tatolab/amos#31"
description: Distribution — one-command install with Claude Code skill setup
dependencies:
  - "up:@github:tatolab/amos#32"
  - "up:@github:tatolab/amos#30"
adapters:
  github: builtin
---

@github:tatolab/amos#31

Standard curl | bash install pattern. Downloads the binary from GitHub Releases (built by #32), puts it on PATH, installs the Claude Code skill to ~/.claude/skills/amos.md. Detect platform and arch automatically.

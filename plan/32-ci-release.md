---
whoami: amos
name: "@github:tatolab/amos#32"
description: CI — cross-platform release builds via GitHub Actions
dependencies:
  - "down:@github:tatolab/amos#31"
adapters:
  github: builtin
---

@github:tatolab/amos#32

Standard cargo cross-compilation action. Use `cross` for linux-musl. macOS needs both x86_64 and aarch64. Upload artifacts to GitHub Releases on tag push.

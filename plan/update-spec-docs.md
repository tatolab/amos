---
whoami: amos
name: "@github:tatolab/amos#10"
description: Update spec and skill docs to reflect amos as a pure data tool
dependencies:
  - "up:@github:tatolab/amos#1"
  - "up:@github:tatolab/amos#5"
  - "up:@github:tatolab/amos#6"
---

Rewrite `spec/SPEC.md`:
- Remove tags from block format and fields section
- Remove the two-phase Claude pipeline description
- Document amos as a scanner + DAG builder
- Document the output format

Rewrite `skills/amos.md`:
- Remove tags from example block
- Update invocation examples (no prompt arg)
- Describe amos as a data tool that dumps DAG state

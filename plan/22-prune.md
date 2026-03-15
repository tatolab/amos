---
whoami: amos
name: "@github:tatolab/amos#22"
description: Cleanup — removing completed nodes that aren't needed by active work
adapters:
  github: builtin
---

@github:tatolab/amos#22

Delete the source .md file and the .amos-status entry. Don't delete nodes that are upstream of anything ready or in-progress — those provide context.

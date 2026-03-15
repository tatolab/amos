---
whoami: amos
name: test-mixed-adapters
description: Verifying mixed adapter resolution — github + exec in one node
adapters:
  github: builtin
  exec: "@github:tatolab/amos-adapters#exec"
---

Here's the GitHub issue for the prune command we built:

@github:tatolab/amos#22

And here's some generated text from the local system:

@exec:echo "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat."

Both adapters resolved in a single node body.

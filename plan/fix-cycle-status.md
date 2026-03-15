---
whoami: amos
name: "@github:tatolab/amos#2"
description: Fix stack overflow when computing status on cyclic graphs
---

`Dag::compute_status_for_index` recurses into upstream nodes without tracking visited nodes. If the graph has a cycle, this blows the stack.

Fix: add a `HashSet<NodeIndex>` visited set to the recursive status walk. If we hit a visited node, return `Blocked` — a cycle means nothing can resolve to done.

Fixed: added visited set to `compute_status_for_index`.

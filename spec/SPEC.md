# Amos Frontmatter Specification v0.4

## Overview

Amos scans markdown files, finds `whoami: amos` blocks, builds a dependency DAG, and dumps the graph state to stdout. He reads the ship and reports what he sees.

## Block Format

```yaml
---
whoami: amos
name: <identifier>
description: <string>
dependencies:
  - up:<node-name>
  - down:<node-name>
context:
  - <local-path>
  - @github:<owner/repo>[#<path>][@<ref>]
  - @url:<url>
status: done | in-progress
---

Implementation notes, context, everything after the closing `---`.
```

Each `.md` file contains at most one amos block, which must be the first frontmatter block in the file. Leading blank lines are allowed.

## Fields

### `whoami` (required)

Must be `amos`. The discriminator — blocks without it are ignored.

### `name` (required)

Unique node identifier. Kebab-case recommended.

### `description` (optional)

Plain-language description of the work item.

### `dependencies` (optional)

- `up:<name>` — upstream dependency (must complete first)
- `down:<name>` — downstream dependent (waits for this)

Both sides can declare the same edge. Duplicates are deduplicated.

### `context` (optional)

- Bare path: local file relative to scan root
- `@github:<owner/repo>[#<path>][@<ref>]`: GitHub reference
- `@url:<url>`: arbitrary URL

### `status` (optional)

`done` or `in-progress`. If absent, computed from DAG.

## Status Computation

1. `status: done` → **done**
2. `status: in-progress` → **in-progress**
3. All upstream done → **ready**
4. Any upstream not done → **blocked**

## CLI

```
amos              # scan cwd, print DAG state
amos --dir /path  # scan specific directory
```

Output goes to stdout. Node count goes to stderr.

## Discovery

Scans all `.md` files under scan root, respecting `.gitignore`. `.git` is skipped.

Each `.md` file contains at most one amos block, which must be the first frontmatter block in the file.

## Output

The DAG state is printed as structured markdown:

- **DAG section** — one line per node: name, status, description, dependencies
- **Execution order** — topologically sorted remaining work
- **Critical path** — longest dependency chain
- **Ready / In-Progress** — expanded detail with full body for actionable nodes
- **Issues** — validation problems (cycles, missing dependencies)

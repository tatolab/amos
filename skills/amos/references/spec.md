# Amos Frontmatter Specification v0.5

## Overview

Amos scans markdown files, finds `whoami: amos` blocks, builds a dependency DAG, and dumps the graph state to stdout. He reads the ship and reports what he sees.

## Block Format

```yaml
---
whoami: amos
name: "@github:tatolab/myproject#42"
description: Short routing hint — enough to know when this node is relevant
dependencies:
  - "up:@github:tatolab/myproject#41"
  - "down:@github:tatolab/myproject#43"
context:
  - path/to/relevant/file.rs
  - @github:org/repo#path/to/file
---

@github:tatolab/myproject#42

Agent-specific instructions, implementation notes, constraints.
The @reference above lazily pulls the issue body at expansion time.
Local content here supplements what the issue says.
```

Each `.md` file contains at most one amos block, which must be the first frontmatter block in the file. Leading blank lines are allowed.

## Fields

### `whoami` (required)

Must be `amos`. The discriminator — blocks without it are ignored.

### `name` (required)

Unique node identifier. Can be a plain name (`my-feature`) or a URI (`@github:owner/repo#N`). URI-based names are self-documenting and portable — they work without amos installed.

### `description` (optional)

A routing hint — tells agents and humans *when this node is relevant*, not what it contains. Think of it like a skill description: enough to decide "should I look at this?" without loading the full body.

Keep it short (one line). The actual content lives in the body, resolved lazily from `@` references.

### `dependencies` (optional)

- `up:<name>` — upstream dependency (must complete first)
- `down:<name>` — downstream dependent (waits for this)

Both sides can declare the same edge. Duplicates are deduplicated.

### `context` (optional)

- Bare path: local file relative to scan root
- `@github:<owner/repo>[#<path>][@<ref>]`: GitHub reference
- `@url:<url>`: arbitrary URL

## Node Body

The body (everything after the closing `---`) contains two kinds of content:

1. **`@` references** — lines starting with `@scheme:reference` are lazily resolved through adapters when the node is expanded. They pull in external content (issue descriptions, file contents, images, video frames).

2. **Local content** — everything else. Agent-specific instructions, implementation constraints, notes that shouldn't live in the external system.

This separation means: the issue has the *what*, the local markdown has the *how-for-AI*.

### Built-in `@` reference schemes

| Scheme | Example | Resolves to |
|--------|---------|-------------|
| `@github:` | `@github:tatolab/amos#11` | GitHub issue title + body |
| `@file:` | `@file:src/main.rs` | File contents (text inline, images as paths) |
| `@url:` | `@url:https://example.com/doc.md` | Downloaded content (images cached locally) |
| `@ffmpeg:` | `@ffmpeg:recordings/bug.mp4` | Keyframes extracted as images |

External adapters can be registered in `.amosrc.toml` for any scheme.

## Status

Status is stored in `.amos-status` at the scan root, not in frontmatter. Node files are immutable specs.

```
- [x] @github:tatolab/amos#1
- [~] @github:tatolab/amos#11
- [ ] @github:tatolab/amos#16
```

`[x]` = done, `[~]` = in-progress, `[ ]` or absent = not started.

### Status Computation

1. `.amos-status` says done → **done**
2. `.amos-status` says in-progress → **in-progress**
3. All upstream done → **ready**
4. Any upstream not done → **blocked**

### Syncing Status

`amos sync` resolves URI-based node names through adapters and writes `.amos-status`. For `@github:` nodes, closed issues → done, open with "in-progress" label → in-progress.

## CLI

```
amos                    # scan cwd, print DAG state
amos --dir /path        # scan specific directory
amos done <node>        # mark node done in .amos-status
amos start <node>       # mark node in-progress
amos reset <node>       # clear node status
amos sync               # sync status from external adapters
```

Output goes to stdout. Status messages go to stderr.

## Discovery

Scans all `.md` files under scan root, respecting `.gitignore`. `.git` is skipped.

## Output

The DAG state is printed as structured markdown:

- **DAG section** — one line per node: name (linkified for `@github:`), status, description, dependencies
- **Execution order** — topologically sorted remaining work
- **Critical path** — longest dependency chain
- **Ready / In-Progress** — expanded detail with lazily resolved body for actionable nodes
- **Issues** — validation problems (cycles, missing dependencies)

Compact for done/blocked nodes (one line). Expanded for ready/in-progress (full body with `@` references resolved).

## Adapters

Adapters resolve URI schemes. Built-in adapters are always available. External adapters are registered in `.amosrc.toml`:

```toml
[adapters.jira]
command = "npx @openclaw/amos-adapter-jira"
```

External adapters are any executable that speaks the protocol:
```
<command> resolve <reference>  → JSON to stdout
<command> batch <json-array>   → JSON object keyed by reference
```

JSON shape:
```json
{
  "name": "optional string",
  "description": "optional string",
  "status": "done | in-progress | null",
  "body": "optional string"
}
```

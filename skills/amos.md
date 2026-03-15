---
name: amos
description: >
  Activate when you see markdown files with amos frontmatter blocks
  (--- with whoami: amos), when the user mentions work streams, DAG
  status, dependency tracking, or wants to work on a planned task.
---

# Amos Work Stream Skill

Amos scans `.md` files for `whoami: amos` blocks, builds the dependency
graph, and dumps the state. He's a mechanic — reads the ship, reports
what he sees.

## Invoking Amos

```bash
amos                # scan cwd, print DAG state
amos --dir /path    # scan specific directory
```

Output goes to stdout — structured markdown with the DAG summary,
execution order, and expanded detail for ready/in-progress nodes.

## Amos Block Format

```yaml
---
whoami: amos
name: node-id
description: What this work item is
dependencies:
  - up:upstream-dep
  - down:downstream-dep
context:
  - path/to/relevant/file.rs
  - @github:org/repo#path/to/file
  - @url:https://docs.example.com
status: done
---

Implementation notes, context, whatever belongs here.
Everything after the closing `---` is the node body. Each file contains at most one amos block.
```

`whoami: amos` is required — it's how amos knows a block is his.
Blocks without it are ignored.

## Status Rules

- **done** = `status: done` set in frontmatter
- **in-progress** = `status: in-progress` set in frontmatter
- **ready** = all upstream deps are done
- **blocked** = any upstream dep is not done

## Using the Output

The output is structured markdown — readable by both humans and LLMs.
Pipe it into other tools, feed it to claude, or just read it.

When work on a node is complete, edit the source `.md` file and set
`status: done` in that node's frontmatter. Next time amos runs, the
DAG updates and downstream nodes become ready.

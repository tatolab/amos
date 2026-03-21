# Amos Node Format

```yaml
---
whoami: amos
name: "<identifier or @github:owner/repo#N>"
description: "<routing hint — when is this node relevant>"
dependencies:
  - "up:<upstream-node-name>"
  - "down:<downstream-node-name>"
adapters:
  github: builtin
---

@github:owner/repo#N

Local agent instructions below the reference.
```

- `name` — plain identifier or `@scheme:reference` URI
- `description` — routing hint, not content
- `dependencies` — `up:` must complete first, `down:` waits for this
- `adapters` — declares what resolvers are needed (`builtin` or `@github:org/repo#path`)
- Body — `@scheme:reference` lines resolve through adapters, everything else passes through

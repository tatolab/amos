---
whoami: amos
name: "@github:tatolab/amos#24"
description: Agent authoring — creating nodes and wiring dependencies from CLI
adapters:
  github: builtin
---

@github:tatolab/amos#24

Add a `Create` variant to `cli.rs`. The command should generate the `.md` file with proper frontmatter — whoami, name, description, dependencies, adapters. Write to `plan/<N>-<slug>.md`. If the name is a `@github:` URI, optionally create the issue via `gh issue create` and use the returned number. Look at how `write_status` works for the file writing pattern.

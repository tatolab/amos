---
whoami: amos
name: strip-tags
description: Remove the tags field from the amos schema — parsed but never used
dependencies:
  - down:update-spec-docs

---

Tags are declared in the spec, parsed in `parser.rs`, echoed in `output.rs`, but nothing filters, groups, or queries by them. Dead weight.

Remove from:
- `parser.rs` — drop `tags` from `RawFrontmatter` and `Node`
- `output.rs` — remove the `tags:` line from prompt output
- `dag.rs` — remove `tags: Vec::new()` from test helper

Keep the serde default so old files with `tags:` don't break parsing — just ignore the field silently.

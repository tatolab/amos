---
name: amos-file
description: >
  File a new GitHub issue with a well-shaped body, the right milestone, and native
  dependency relationships, from a short natural-language intent. Use when the user
  says "file this", "open an issue for X", "create a ticket", "log this as a bug",
  when discussion surfaces a new task that should be tracked, or after the
  assistant offered to file something and the user confirmed. Infers milestone
  and labels; uses AskUserQuestion whenever confidence is low.
allowed-tools: Bash, Read, Write, AskUserQuestion, Glob, Grep
---

# Filing a new issue

Your job is to produce a consistent, well-shaped GitHub issue from minimal input,
so the user never has to hand-author a full template. You draft; the user approves
via `AskUserQuestion`; amos executes atomically.

Never skip the approval gate. Never execute a create on a draft the user hasn't
seen in full.

## Step 1 — Capture intent

Use whatever the user already gave you:

- An explicit request ("file an issue for the Python regex bug")
- A conversation thread where a bug/task surfaced
- A prior assistant prompt ("want me to file this?") + the user saying yes

If the intent is too vague to produce a draft, ask one concrete question via
`AskUserQuestion` — e.g. "Should this be a bug report, a feature request, or a
research task?" — rather than guessing and generating a bad draft.

## Step 2 — Discover the template

Look, in order:

1. `docs/issue-template.md` in the project root.
2. `.github/ISSUE_TEMPLATE/*.md` (pick the closest match to intent, or ask
   if multiple look relevant).
3. Nothing found → use the amos default below.

Read the file. Follow its section headers verbatim. Don't invent new sections.

### Amos default template

```markdown
## Description
One short paragraph, written for an AI agent with no prior context.

## Context
Why this matters. Constraints, prior work, related discussion.

## Exit criteria
- [ ] <concrete deliverable>
- [ ] <concrete deliverable>

## Tests / validation
- [ ] <inline test case>  OR  "Blocked by #N (test harness)"

## Related
- Milestone: <name>
- See also: #N, #M
```

**Do NOT** put `Blocked by:` / `Blocks:` in the `Related` section — those are
set via native GitHub relationships, not text. The `Related` section is for
human context only ("see also", "context from #N", etc.).

## Step 3 — Draft title + body

Title rules:

- **Conventional-commit prefix** where appropriate: `fix(python): ...`,
  `feat(adapter): ...`, `chore(ci): ...`. Match the project's existing title
  style — scan `gh issue list --limit 20 --state all --json title` if unsure.
- Under 70 characters.
- No trailing period.
- Describes *what breaks* or *what ships*, not *how to fix it*.

Body rules:

- Fill every template section. Empty = drift.
- Be terse. One short paragraph per section. Use lists where the template has them.
- Write for an agent walking in cold, not for someone who saw the conversation.
- If the intent came from a debugging thread, copy the exact error strings / VUIDs
  / stack traces into `Context` — they're search-fuel for future lookups.

## Step 4 — Infer milestone (with AskUserQuestion fallback)

```bash
"$HOME/.local/bin/amos" milestones --json --dir <project-root> | jq '.milestones[] | .title'
```

Match against the draft's title + body keywords + label affinity (e.g. `python`
label → "Polyglot SDK Realignment" milestone on streamlib). **Confidence tiers:**

- **High** — one milestone's title or scope contains a core concept from the
  draft (exact word match on a distinctive term). Use it.
- **Medium / low / ambiguous** — two or more candidates look plausible, or
  none clearly match. Call `AskUserQuestion`:

  ```
  Which milestone should this go in?
  [ 1 ] <candidate 1 title>
  [ 2 ] <candidate 2 title>
  [ 3 ] <candidate 3 title>
  [ 4 ] None of these / I'll tell you
  [ 5 ] No milestone (orphan)
  ```

  If the user picks "I'll tell you", ask for the title and validate it against
  the `amos milestones` list.

## Step 5 — Infer labels

```bash
gh label list --repo <owner>/<repo> --limit 100 --json name,description
```

Apply labels whose name or description matches draft keywords:

- `bug` if the draft describes broken behavior (not "rough edge" or "nice to have")
- Platform tags (`linux`, `macos`, `polyglot`) if the draft mentions them
- Scope tags (`ci`, `docs`, `research`) for obvious fits

If unsure for a label, skip it. Labels are cheap to add after filing; wrong
labels cause routing confusion.

## Step 6 — Detect relationships

Scan the user's intent + conversation for explicit signals:

- "this blocks #322", "depends on #310", "after #300 lands"
- "sub-issue of #319", "part of the #319 umbrella"
- "related to #302" → goes in the `Related` section as plain text, not a native
  relationship

For each explicit mention, turn into a structured edge:

```json
{
  "blocked_by": ["@github:<owner>/<repo>#310"],
  "blocks": ["@github:<owner>/<repo>#322"],
  "sub_issue_of": "@github:<owner>/<repo>#319"
}
```

If the intent implies a dependency but doesn't say the direction clearly, ask
via `AskUserQuestion` — wrong-direction edges are painful to unwind.

## Step 7 — Approval gate (mandatory)

Present the draft in full, then `AskUserQuestion`:

```
Ready to file this issue?

Title:      <title>
Milestone:  <milestone or "none">
Labels:     <label1, label2, ...>
Blocked by: <list, or "none">
Blocks:     <list, or "none">
Parent:     <sub_issue_of, or "none">

---
<body>
---

[ 1 ] File it as shown
[ 2 ] Change the title
[ 3 ] Change the milestone
[ 4 ] Edit the body
[ 5 ] Fix the relationships
[ 6 ] Cancel
```

If the user picks anything but "File it as shown", iterate on the relevant
field and re-show the full draft before asking again.

## Step 8 — Execute atomically

Write the approved draft to a temp JSON spec, then hand off to the binary:

```bash
cat > /tmp/amos-spec.json <<'JSON'
{
  "title": "fix(python): ...",
  "body": "## Description\n...",
  "milestone": "Polyglot SDK Realignment",
  "labels": ["bug", "polyglot"],
  "blocked_by": ["@github:tatolab/streamlib#322"],
  "blocks": [],
  "sub_issue_of": null
}
JSON

"$HOME/.local/bin/amos" issue-create --spec /tmp/amos-spec.json --dir <project-root>
rm /tmp/amos-spec.json
```

The binary creates the issue, applies the milestone + labels, then sets every
native relationship in one pass. On success it prints `amos: created
@github:<owner>/<repo>#<N> — <url>`.

If any relationships failed to apply (non-existent target, API hiccup), the
binary reports them to stderr — the issue is still created; re-run `amos
sync-edges` or file a fix-up.

## Step 9 — Report back

Tell the user, in English:

- Issue number + URL
- Milestone, labels
- Relationships created
- Anything that needed a retry

Don't summarize the body back at the user — they already approved it.

## Common mistakes to avoid

- **Don't** put `Blocked by: #N` text in the body's `Related` section when you
  also added the native relationship. Native edges only. The `Related` section
  is for free-text context.
- **Don't** skip the approval gate even if the draft feels obviously right.
  Users trust this pattern because it's predictable.
- **Don't** invent a milestone that doesn't exist. Validate every `milestone:`
  value against `amos milestones` before handing to the binary.
- **Don't** hardcode a repo. The binary auto-detects from the scan root's git
  remote; pass `--dir <project-root>` so it picks the right one.
- **Don't** create a local plan file. AI-specific notes belong in the issue
  body. The plan-file path is deprecated for new issues.

---
name: amos-file
description: >
  File a new GitHub issue with a well-shaped body, the right milestone, and native
  dependency relationships, from a short natural-language intent. Use when the user
  says "file this", "open an issue for X", "create a ticket", "log this as a bug",
  when discussion surfaces a new task that should be tracked, or after the
  assistant offered to file something and the user confirmed. Infers milestone
  and labels; asks in chat whenever confidence is low.
allowed-tools: Bash, Read, Write, Glob, Grep
---

# Filing a new issue

Your job is to produce a consistent, well-shaped GitHub issue from minimal input,
so the user never has to hand-author a full template. You draft; the user approves
in chat; amos executes atomically.

Never skip the approval gate. Never execute a create on a draft the user hasn't
seen in full.

## Approval gates: ask in chat, do not call AskUserQuestion

Every gate in this skill — disambiguation questions, milestone selection, type
selection, relationship direction, final pre-file approval — is a **plain-text
question in the chat**, not an `AskUserQuestion` tool call.

Reasons:
- `AskUserQuestion` holds a permission stream open while waiting for the user
  to click. Long-running drafts (issue bodies of several thousand chars) plus
  human deliberation time routinely exceed the stream's idle timeout, killing
  the session with `Tool permission stream closed before response received`.
  This is especially common when driving Claude Code remotely (Telegram bridge,
  SDK CLI), and it loses the entire drafting context.
- A text question costs nothing — the harness handles the wait naturally as
  part of normal turn-taking. Each user reply is a fresh inbound message, no
  held-open IPC.

**Mechanics for every gate below:**
1. Print the question and the numbered options as plain Markdown.
2. End your turn — do not call any further tool, do not narrate "waiting for
   your reply".
3. The user's next message is the answer. Parse it (a bare number, a label
   from the option list, or free-form text) and continue.

## Step 1 — Capture intent

Use whatever the user already gave you:

- An explicit request ("file an issue for the Python regex bug")
- A conversation thread where a bug/task surfaced
- A prior assistant prompt ("want me to file this?") + the user saying yes

If the intent is too vague to produce a draft, ask one concrete question in
chat — e.g. "Should this be a bug report, a feature request, or a research
task?" — rather than guessing and generating a bad draft. Use the chat-gate
mechanics above (plain text, end turn, wait for reply).

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

<!-- amos:ai-notes-begin -->
## AI Agent Notes

Agent-facing context that doesn't belong in the human-readable sections
above. Include only what's useful to a future agent walking in cold:

- Exact error strings, VUIDs, stack traces from the conversation that
  led to this issue (search-fuel for lookups).
- Concrete file paths + line numbers relevant to the fix.
- Approaches already tried or ruled out, with the reason.
- Hidden constraints or invariants the code enforces.
- Pointers to adjacent amos nodes or docs (`@github:owner/repo#N`,
  `docs/learnings/foo.md`).

Skip anything already obvious from the Description or Exit criteria —
no duplication. If there's nothing agent-specific to add, leave an
explicit "None." so a future reader knows it wasn't forgotten.
<!-- amos:ai-notes-end -->
```

**Do NOT** put `Blocked by:` / `Blocks:` in the `Related` section — those are
set via native GitHub relationships, not text. The `Related` section is for
human context only ("see also", "context from #N", etc.).

**Always include the `AI Agent Notes` section** — even on issues without
obvious agent-specific context. It's the contract between human-filed
issues and agents picking them up later. The HTML markers
(`<!-- amos:ai-notes-begin -->` / `<!-- amos:ai-notes-end -->`) are
load-bearing: they let amos tooling find and update the section without
risking the rest of the body.

### Project-specific templates

If the project has its own `docs/issue-template.md` or
`.github/ISSUE_TEMPLATE/*.md`, use its sections verbatim — **but if it
doesn't already include an AI Agent Notes section, append one at the end
of the body** using the marker-wrapped shape above. The section is
non-negotiable; it's how agents and humans share context on a remote-first
issue.

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

## Step 4 — Infer milestone (with chat fallback)

```bash
"$HOME/.local/bin/amos" milestones --json --dir <project-root> | jq '.milestones[] | .title'
```

Match against the draft's title + body keywords + label affinity (e.g. `python`
label → "Polyglot SDK Realignment" milestone on streamlib). **Confidence tiers:**

- **High** — one milestone's title or scope contains a core concept from the
  draft (exact word match on a distinctive term). Use it.
- **Medium / low / ambiguous** — two or more candidates look plausible, or
  none clearly match. Ask in chat (per the gate mechanics in the intro):

  ```
  Which milestone should this go in?

  1. <candidate 1 title>
  2. <candidate 2 title>
  3. <candidate 3 title>
  4. None of these — I'll tell you the title
  5. No milestone (orphan)
  ```

  Then end your turn. If the user picks "I'll tell you", validate the title
  they give against the `amos milestones` list before continuing.

## Step 4.5 — Infer the issue type

GitHub has three repo-level issue types: **Bug**, **Feature**, **Task**.
They're separate from labels and render with distinct icons in the UI.
Amos passes `issue_type` through to `gh`'s `updateIssueIssueType`
mutation — if the repo has the type configured, the new issue picks
it up.

Fetch available types (usually this is the standard three, but
projects sometimes add more):

```bash
gh api graphql -f query='{
  repository(owner:"<owner>",name:"<repo>") {
    issueTypes(first:10) { nodes { name description } }
  }
}' | jq '.data.repository.issueTypes.nodes[]'
```

Infer from the draft:

- **Bug** — describes a thing that is broken, throws an error, crashes,
  fails a test, produces wrong output, regresses from a prior working
  state. Conventional-commit prefix `fix(...)`.
- **Feature** — a new capability that doesn't exist yet. Not a fix, not
  maintenance. Conventional-commit prefix `feat(...)`.
- **Task** — everything else. Chores, refactors, docs, research,
  rollup retests, CI work, dependency bumps. Conventional-commit
  prefixes `chore(...)`, `refactor(...)`, `docs(...)`, `test(...)`,
  `perf(...)`, or no prefix for umbrellas and research tickets.

If confidence is high (conventional-commit prefix matches cleanly),
just set the type. If the draft title has no prefix and the content is
ambiguous (e.g., "improve the caching story"), ask in chat (per the gate
mechanics in the intro) and end your turn:

```
What type of issue is this?

1. Bug — something is broken
2. Feature — new capability
3. Task — maintenance, refactor, research, etc.
```

If the repo doesn't have any issue types configured, omit the field —
the binary will skip the mutation cleanly.

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

**Only four relationship types go into native GitHub relations.** If a
reference doesn't fit one of these, it's free-text context — don't try
to force it into a native edge.

| Intent | Native relation | Pattern to match |
| --- | --- | --- |
| A can't start until B is done | `blocked_by` on A | "blocked by #N", "depends on #N", "waits on #N", "after #N lands" |
| A must land before B can start | `blocks` on A | "this blocks #N", "gates #N", "prerequisite for #N" |
| A is a concrete child under umbrella B | `sub_issue_of` on A | "sub-issue of #N", "part of the #N umbrella", "child of #N", "Parent: #N" |
| A is the same issue as B | `duplicate` of (not yet supported by amos) | "duplicate of #N", "duplicates #N" |

**Everything else stays as free-text in the `Related` section** — or
gets dropped entirely. Phrases like "exposed by", "surfaced by",
"follow-up to", "see also", "related to", "in the same cluster as"
are soft references with no GitHub-native equivalent. If they don't
affect work order, don't call them out at all.

Rule of thumb: **if a reference doesn't fit the table above, it's
probably not a relationship worth calling out.** The value of a native
relation is that `amos next` / `amos blocked` will respect it and
order work correctly. Free-text refs are just noise the next agent
has to mentally filter.

For each matching reference, turn it into a structured edge in the
spec:

```json
{
  "blocked_by": ["@github:<owner>/<repo>#310"],
  "blocks": ["@github:<owner>/<repo>#322"],
  "sub_issue_of": "@github:<owner>/<repo>#319"
}
```

If the intent implies a dependency but doesn't say the direction
clearly, ask in chat (per the gate mechanics in the intro) and end
your turn — wrong-direction edges are painful to unwind (they create
cycle errors on any later correction).

## Step 7 — Approval gate (mandatory, in chat)

Present the draft in full as plain Markdown in your message, then list the
options and **end your turn**. Do NOT call `AskUserQuestion` — see the
intro's "Approval gates" section for why.

```
Ready to file this issue?

Title:      <title>
Type:       <issue_type or "none">
Milestone:  <milestone or "none">
Labels:     <label1, label2, ...>
Blocked by: <list, or "none">
Blocks:     <list, or "none">
Parent:     <sub_issue_of, or "none">

---
<body>
---

Reply with one of:

1. File it as shown
2. Change the title
3. Change the type
4. Change the milestone
5. Edit the body — name the section
6. Fix the relationships
7. Cancel

Or describe the change you want in your own words.
```

After printing the options, end your turn — no further tool calls. The
user's next message is the answer. If they pick anything but option 1,
iterate on the relevant field and re-show the full draft before asking
again.

## Step 8 — Execute atomically

Write the approved draft to a temp JSON spec, then hand off to the binary:

```bash
cat > /tmp/amos-spec.json <<'JSON'
{
  "title": "fix(python): ...",
  "body": "## Description\n...",
  "issue_type": "Bug",
  "milestone": "Polyglot SDK Realignment",
  "labels": ["polyglot"],
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

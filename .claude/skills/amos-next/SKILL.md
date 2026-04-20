---
name: amos-next
description: >
  Pick up and execute the next ready-to-start issue from the focused milestone,
  end-to-end: announce, gate on user confirmation, branch, do the work, run
  the tests listed in the issue body, open a PR, report. This is the skill to
  invoke when the user says "continue", "next task", "what's next", "pick up
  where we left off", "work on the next issue", "keep going", or similar. It
  replaces any project-local PROMPT.md protocol.
allowed-tools: Bash
---

# Task execution protocol — `/amos:next`

You are executing the next ready-to-start issue from the focused milestone.
The protocol below is authoritative — follow it in order.

## Prereqs

- The project's `CLAUDE.md` has already been loaded into context — follow it.

## Step 0 — Focus triage

Check whether a milestone is currently focused and whether it still has
actionable work:

```bash
"$HOME/.local/bin/amos" focus --dir <project-root>        # prints current focus, or "no milestone currently focused"
"$HOME/.local/bin/amos" milestones --dir <project-root>   # per-milestone open / ready / done counts
```

Handle these four cases:

**(a) No focus set.** Run `amos milestones`, present a ranked list of
candidate milestones (see *Ranking* below), and wait for the user to pick
via `/amos:focus "<title>"`. Do NOT pick for them.

**(b) Focus is set but has `0 open`.** The milestone is done. Congratulate
the user, suggest closing it in GitHub (`gh milestone close <number>` — use
`gh api repos/<owner>/<repo>/milestones` to find the number), then present
ranked candidates for the next milestone.

**(c) Focus is set, `open > 0` but `ready == 0`.** The milestone's remaining
work is blocked. Run `amos blocked` to see the chain and, for each blocked
item, its open blockers. Then either:
- If the gating blocker is in another milestone, recommend `/amos:focus`
  to that milestone so the user can unstick this one.
- If the chain is internal, recommend the earliest blocker-free item (or
  explain the circular/deadlocked case if any).

**(d) Focus is set, `ready >= 1`.** Continue to Step 1 below.

## Ranking recommendations

When proposing milestones, use this heuristic (sort descending):

1. **Exclude** anything disqualified — milestones whose every open item is
   labeled `frozen`, or whose description starts with `[BLOCKED` or contains
   "do not start". Call these out separately as "deferred / blocked —
   skipping."
2. **Prefer** higher `ready / open` ratio — milestones where most open
   items can start today.
3. **Prefer** smaller `open` counts — a 1- or 2-item milestone closes fast
   and gives a real deliverable.
4. **Prefer** milestones whose items are `blocked_by` the focused milestone
   not at all (i.e. standalone) or that unblock other milestones (items in
   other milestones' `blocked_by` pointing at this one).
5. **Tiebreak** alphabetical.

Emit 3–5 candidates. For each, one line on why-to-start or why-to-defer.
Conclude with: "Run `/amos:focus "<title>"` to pick one."

## Step 1 — Find the next issue (inside the focused milestone)

```bash
"$HOME/.local/bin/amos" next --dir <project-root>
```

If there's more than one ready item, **ask the user to pick** — don't choose
for them. Show each with its `#N — title` line.

## Step 2 — Load context for the chosen issue

For the chosen `<issue>` (e.g. `@github:tatolab/streamlib#326`):

```bash
# Full issue: description, exit criteria, tests, comments, labels.
gh issue view <N> --repo <owner>/<repo> --json title,body,state,labels,milestone,comments

# Optional: any local AI notes in an amos plan file.
"$HOME/.local/bin/amos" show "<issue>"
```

Then, for each label on the issue, look for a matching workflow file in the
project's `.claude/workflows/` directory:

```bash
ls <project-root>/.claude/workflows/
# If <label>.md exists, read it — it carries specialty context for that kind
# of work (e.g. video-e2e.md, ci.md, macos.md).
```

Read every matching workflow file into context before starting.

## Step 3 — Announce + gate on confirmation

Emit exactly this block, filled in:

```
## Starting Task

- **Issue**: #<N> — <title>
- **Milestone**: <milestone title>
- **Labels**: <label1>, <label2>
- **Loaded workflows**: <list of .claude/workflows/*.md read, or "none">
- **Branch**: `<branch-name>`
- **Summary**: <1–2 sentence plan from the issue Description>
- **Exit criteria**: <count, from issue Exit criteria section>
- **Tests to run as gate**: <count, from issue Tests/validation section>
- **Files in scope**: <list — from issue + workflow context>
- **Estimated scope**: small | medium | large

Proceed? [y/n]
```

**Wait for explicit user confirmation.** Do not proceed on silence.

## Step 4 — Branch

```bash
git checkout main && git pull origin main
git checkout -b <branch-name>
```

Branch name convention: `<type>/<slug>-<issue-num>` where `<type>` is
`feat` / `fix` / `refactor` / `docs` / `test` / `chore` depending on
the issue. Use the shortest slug that's still readable.

## Step 5 — Do the work

- Scope: strictly the issue's Exit criteria. Anything else → note as a
  follow-up, do not touch.
- Honor every rule in `CLAUDE.md` and in the loaded workflow files.
- `cargo check` (or project-specific equivalent) frequently.
- Conventional commits (`feat:`, `fix:`, etc.), one logical change per
  commit.
- Never commit broken code. If a commit would be broken, fold the fix in
  before committing.

## Step 6 — Run the test gate

For each bullet in the issue's **Tests / validation** section, run the
corresponding command. Report results in this block:

```
## Test Results

- **cargo check**: ✅ | ❌
- **cargo test <pattern>**: ✅ N passed | ❌ N failed
- **<E2E or other workflow-driven test>**: ✅ | ❌ | ⏭ skipped (reason)

### Issues found
- <any, or "None">
```

If any gate fails, **fix it before opening the PR**. If a listed test
cannot be run in this environment (e.g. needs a GPU CI), explicitly flag
it as skipped and call out that the PR needs that check when CI runs.

## Step 7 — Push + open PR

```bash
git push -u origin <branch-name>
gh pr create --title "<conventional-commit-style>" --body "$(cat <<'EOF'
## Summary

<1–3 bullets — what changed and why>

## Closes

Closes #<N>

## Exit criteria

<checklist copied from the issue body, with items checked off>

## Test plan

<checklist from the issue's Tests/validation section, with results>

## Follow-ups

<list of out-of-scope things discovered, or "None">

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

## Step 8 — Report back

```
## Task Complete

- **Issue**: #<N> — <title>
- **Branch**: `<branch-name>`
- **PR**: <url>
- **Commits**: <n>
- **Files changed**: <n>
- **Lines**: +<added> / -<removed>

### Tests run
- <summary>

### Follow-ups filed
- <list, or "None">

### Ready for review
PR is open. Do NOT merge — merge is the user's call.
```

## Rules (non-negotiable)

1. **One branch per issue.** Never mix work from multiple issues.
2. **Never merge to main.** PRs only.
3. **Never edit outside scope.** Note as follow-up.
4. **Always announce and wait for confirmation** before branching.
5. **Always run the test gate** before pushing.
6. **Never ignore a label's workflow file** — if `.claude/workflows/<label>.md`
   exists for a label on this issue, its instructions are mandatory for
   this task.
7. **Bank follow-ups as new issues** when the user asks — include the same
   `Description / Context / Exit criteria / Tests / Related` template and
   assign to a milestone.

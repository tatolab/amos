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
Prefer `--json` on every `amos` command; pipe through `jq` to extract fields;
craft human-readable summaries for the user in natural language. Never echo
raw JSON at the user.

## Prereqs

- The project's `CLAUDE.md` has already been loaded into context.

## Step 0 — Focus triage

```bash
"$HOME/.local/bin/amos" focus --json --dir <project-root>              # current focus
"$HOME/.local/bin/amos" milestones --json --dir <project-root>         # per-milestone counts
```

Four cases:

**(a) `focus: null`** → no milestone picked. List candidates (see
*Ranking* below) and stop. User picks via `/amos:focus`.

**(b) Focused milestone has `open == 0`** → finished. Congratulate, tell
the user to close it in GitHub (`gh milestone close <number>` — find
number via `gh api repos/<owner>/<repo>/milestones`), and rank candidates
for the next milestone.

**(c) Focus open > 0 but `ready == 0`** → blocked chain. Run
`amos blocked --json` to show what's gating. For each blocked node, its
`blocked_by` lists the open blockers. If any blocker is in a different
milestone, recommend `/amos:focus` there. If the chain is internal,
show it and stop.

**(d) `ready >= 1`** → continue to Step 1.

## Ranking recommendations (used in a/b)

Parse `amos milestones --json` and filter open milestones. Rank:

1. **Exclude**: every node frozen (`labels_local` contains "frozen") OR
   description starts with `[BLOCKED` or contains "do not start".
2. **Prefer** high `ready / open` ratio.
3. **Prefer** smaller `open` (faster close-out).
4. **Prefer** milestones unblocking other milestones (peek at each
   candidate's nodes' `blocks:` — targets that point into another open
   milestone's ready items).
5. **Tiebreak** alphabetical.

Emit 3–5 candidates. For each, one line on *why start* / *why defer* in
natural English. Conclude with: "Run `/amos:focus \"<title>\"` to pick
one."

## Step 1 — Find the next issue

```bash
"$HOME/.local/bin/amos" next --json --dir <project-root> \
  | jq -r '.nodes[] | "\(.name)\t\(.description)"'
```

If `.count == 1`, pick it. If multiple, present them as a short list in
English and ask the user to pick. Don't decide for them.

## Step 2 — Load context for the chosen issue

For the chosen `<issue>` (e.g. `@github:tatolab/streamlib#347`):

```bash
# Full issue content from GitHub
gh issue view <N> --repo <owner>/<repo> --json \
  title,body,state,labels,milestone,comments

# Any local AI notes in an amos plan file
"$HOME/.local/bin/amos" show <issue> --json \
  | jq -r '.body // ""'
```

Extract labels from the issue JSON (`.labels | map(.name)`). For each
label, check if `<project-root>/.claude/workflows/<label>.md` exists;
read every matching file into context. Those specialty workflows carry
mandatory rules for the kind of work (`ci`, `video-e2e`, `macos`,
`polyglot`, `research`, etc.).

## Step 2.5 — Verify against current state (staleness check)

**Issues are goals, not specs.** The body — exit criteria, suggested
file paths, AI Agent Notes, ruled-out approaches, related-issue
references — captures the best understanding when the issue was
filed. Code lands, dependencies close, architectures shift; by the
time you pick the issue up, parts of the body may be stale.

Before announcing a plan, audit the issue body against current code
and current issue state. For each load-bearing claim in the body,
check:

- **Referenced files / code paths** still exist in the shape claimed.
  If the body says "edit X in module Y," confirm Y still exists and
  X is still where it says.
- **Cited "defects" or "missing features"** are still real. PRs that
  landed since filing may have already addressed them.
- **Suggested follow-up issues** haven't already been filed. If the
  body says "file three implementation issues," check whether
  `gh issue list` already shows them — they may exist with
  `blocked_by: #<this>`.
- **Dependency edges** match GitHub. If the body claims a `Blocked
  by` relationship, confirm it via `amos show`.
- **AI Agent Notes** still reflect reality. Strike-out items that
  are no longer true (per the markdown-editing rules in CLAUDE.md —
  preserve disagreement, don't silently overwrite).

Use the Agent tool with `subagent_type=Explore` for non-trivial
verification across multiple files; for narrow lookups, Grep / Read /
`gh issue view` directly.

If you find staleness, update the issue body in place via
`gh issue edit <N> --body-file ...` with strike-throughs and reasoning
preserved. Do this *before* announcing the plan, so the announcement
references a clean issue body and a future picker doesn't have to
re-derive the same dead ends.

The plan you announce in Step 3 is the **fresh plan**, not a
regurgitation of the issue body. Where the body and current evidence
disagree, the plan goes with current evidence (and explicitly notes
what changed and why).

## Step 3 — Announce + gate on confirmation

Compose a short announcement in English (not JSON). The plan you
announce is the *fresh* plan from Step 2.5 — exit criteria, files in
scope, and tests are what *current state* dictates, not what the
issue body literally says (where the two diverge, lead with the
fresh plan and call out the divergence). Example shape:

```
## Starting Task

- **Issue**: #<N> — <title>
- **Milestone**: <milestone title>
- **Labels**: <labels, or "none">
- **Loaded workflows**: <files read, or "none">
- **Branch**: `<branch-name>`
- **Goal** (paraphrased from issue): <1–2 sentences>
- **Refined plan** (after Step 2.5 staleness check):
  - Exit criteria: <fresh, current-state-aware deliverables>
  - Files in scope: <from current code, not just issue body>
  - Test shape: <from issue + workflows, refined for current state>
- **What changed vs. issue body**: <list of items struck out,
  references that have moved, or "no divergence">
- **Scope estimate**: small | medium | large

Proceed? [y/n]
```

Wait for explicit user confirmation.

## Step 4 — Branch

```bash
git checkout main && git pull origin main
git checkout -b <type>/<slug>-<N>
```

`<type>` = `feat` / `fix` / `refactor` / `docs` / `test` / `chore`
from the conventional-commit family.

## Step 5 — Do the work

- Scope: the **goal as refined in Step 3's plan**, not a literal
  reading of every checkbox in the issue body. If the body says
  "do X, Y, Z" and Step 2.5 found Y is already done, skip Y.
- Note anything genuinely out of scope as a follow-up, don't touch.
- Honor `CLAUDE.md` + every loaded workflow file.
- `cargo check` (or project equivalent) frequently.
- Conventional commits. Never commit broken code.

## Step 6 — Run the test gate

For each bullet in the issue's **Tests / validation** section, run the
command and collect the result. Compose results in English:

```
## Test Results

- cargo check: ✅
- cargo test <pattern>: ✅ N passed
- <E2E / workflow-driven test>: ✅ | ❌ | ⏭ skipped (reason)

### Issues found
- <any, or "None">
```

Don't push until the gate is green. If a listed test can't run in this
environment (no GPU CI, etc.), mark ⏭ skipped with a clear reason and
note that CI must catch it.

## Step 7 — Pre-PR review gate (automatic)

**Do not ask the user "open PR? [y/n]".** Invoke the `pr-review-gate`
skill with the branch's diff and the issue context; use its verdict to
decide what happens next:

- **PASS** — proceed straight to Step 8 (push + open PR). Print one
  line like "Review passed — opening PR."
- **FIX** — apply the listed fixes on this branch, re-run the test
  gate (Step 6), commit with message `review: address pr-review-gate
  feedback`, and re-invoke `pr-review-gate`. Cap the loop at 3
  iterations; on the 4th, treat as DISCUSS.
- **DISCUSS** — surface the reviewer's rationale verbatim and ask the
  user: "Want me to fix these or open the PR anyway?" Do not proceed
  without an explicit answer.

The gate is intentionally narrow — it only decides whether to open the
PR. Merge decisions, follow-up filing, and broader discussion stay
with the user at Step 9.

## Step 8 — Push + open PR

Before creating the PR, collect **every** issue the branch addresses — not just
the primary one from Step 1 — so GitHub auto-closes all of them on merge.

### Detect addressed issues

Gather the set from three sources, then dedup:

1. **Primary issue** from Step 1.
2. **Commit trailers** — scan this branch's commits for explicit close
   keywords:
   ```bash
   git log main..HEAD --pretty=%B \
     | grep -oiE '(closes|fixes|resolves) +#[0-9]+' \
     | grep -oE '#[0-9]+' | sort -u
   ```
3. **Branch name** — if the branch name ends in `-<N>` (e.g.
   `fix/api-server-config-388`), treat that as a primary.

If a commit mentions an issue without a close keyword (just `#N`), that's a
reference, not a closing link — don't auto-close it. If you're unsure whether
an issue should close with this PR, leave it out and file a follow-up; wrong
auto-closes are annoying to reverse.

### Create the PR

```bash
git push -u origin <branch-name>
gh pr create --title "<conventional-commit title>" --body "$(cat <<'EOF'
## Summary
<1–3 bullets>

## Closes
Closes #<N1>
Closes #<N2>
Closes #<N3>

## Exit criteria
<copied from issue body, checked>

## Test plan
<copied from issue Tests/validation, with results>

## Follow-ups
<out-of-scope, or "None">

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

One `Closes #N` per line — GitHub's auto-close parser only fires on that
exact shape. `Closes #1, #2, #3` on one line does NOT work.

## Step 9 — Report back

English summary, not JSON:

```
## Task Complete

- **Issue**: #<N> — <title>
- **Branch**: `<branch-name>`
- **PR**: <url>
- **Commits**: <n> · **Files**: <n> · **Lines**: +<added> / -<removed>

### Tests run
<summary>

### Follow-ups filed
<list, or "None">

### Ready for review
PR is open — merge is the user's call.
```

## Rules (non-negotiable)

1. One branch per issue.
2. Never merge to main.
3. Never edit outside scope.
4. Always announce + wait for confirmation.
5. Always run the test gate before pushing.
6. Every matching `.claude/workflows/<label>.md` is mandatory.
7. Present data to the user in natural English, never raw JSON.

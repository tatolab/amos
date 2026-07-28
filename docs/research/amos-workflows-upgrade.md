# Upgrade path: amos + workflows, without the label coupling

Research report. No code proposed here — this is the shape of the change and the
reasoning behind it.

**Sources read:** `tatolab/streamlib` at `845166f` — `.claude/loops/{README,budget,constraints}.md`,
`.claude/workflows/{milestone-loop,worktree-work,draft-design,run-research}.js`,
`.claude/skills/{work-on-milestone,milestone-loop,work-on-ticket,loop-status}/SKILL.md`,
`.claude/agents/*`, `.claude/rules/flow.md`, `CLAUDE.md`. `tatolab/amos` — `spec/SPEC.md`,
`src/{cli,main,gh_adapter}.rs`, `.claude/skills/amos*/SKILL.md`.

---

## 1. The two systems in one paragraph each

**The loop** is a four-layer reconciler. `/work-on-milestone` resolves a milestone, writes
`focused_milestone` into a gitignored JSON blob, sets a `/goal`, and registers a 30-minute
`CronCreate` heartbeat. Each firing runs the `milestone-loop` skill, which launches
`milestone-loop.js`: an agent reconciles against GitHub, one agent per candidate classifies
work shape and zones, a pure-JS planner picks a conflict-free batch, and each ticket is
dispatched via `workflow()` to one of three level-1 scripts. The main one,
`worktree-work.js`, composes a stage list from shape and zones and walks it — claim,
rederive, implement, zone-specific stages, gates, self-review, verify — then runs up to six
review-and-fix rounds before opening a PR. Nine subagent definitions, five symptom indexes,
nine rules files, four hooks, and a gitignored state file plus run-log support it.

**amos** is a compiled Rust preprocessor over GitHub. It reads issues, milestones, and
GitHub's five *native* typed relationships (`blockedBy`, `blocking`, `parent`, `subIssues`,
duplicate) through one paginated GraphQL call, builds a DAG, and answers structural queries:
`next` (ready to start), `blocked` (with blockers named), `graph`, `milestones`, `validate`,
`show`. `amos focus` scopes all of them to a milestone via `.amosrc.toml`. Local plan files
are *optional* overlays holding AI-only notes; edges live upstream. Status is adapter-derived,
not cached locally.

---

## 2. Where the waste actually is

Counting agent spawns for one ordinary Rust ticket in an ABI zone with three fix rounds:

| Stage | Spawns | Tier |
|---|---|---|
| reconcile (shared per pass) | 1 | sonnet |
| classify | 1 per candidate | sonnet |
| claim | 1 | sonnet |
| rederive | 1 | opus |
| implement | 1 | opus |
| abi-contract | 1 | opus |
| gates | 1 | local-ci-runner |
| self-review | 1 | opus |
| verify round 1 | branch-guard + change-verifier + 1–2 zone lenses + rust-craftsmanship = 4–5 | mostly opus |
| ×3 fix rounds | (fix + branch-guard + change-verifier + gating lenses) × 3 ≈ 15 | mostly opus |
| open-pr | 1 | sonnet |
| record (shared) | 1 | sonnet |

**≈26 spawns, ~20 of them opus, for one ticket.** At `max_worktrees: 4` plus classify and
the shared agents, a single pass reaches ~100–170 spawns — which is why `budget.md` carries a
90-spawn backstop and then tells you not to spend it. This fires every 30 minutes.

Three specific drivers, in order of cost:

**(a) A naming nit costs exactly as much as a correctness blocker.** `worktree-work.js:604`
folds severity with an OR: `hasBlocker || hasReject || hasShouldFix || hasLow → FIX`. `low`
is defined in the taxonomy as "a nit — naming, a doc line." One nit therefore forces a full
fix round, and a fix round re-spawns the change-verifier, which is instructed to "**Still run
the FULL gate battery yourself**" (line 494). So a bad variable name buys you ~5 opus agents
and a complete test-suite re-run. The *intent* is right and stated well ("a human's attention
is the scarce resource here"). The mechanism is what's expensive.

**(b) Nothing carries over between rounds.** Every fix round spawns a *fresh* implementer that
receives `JSON.stringify(reviews)` and re-derives the entire change from the worktree.
Every re-verifying lens is likewise fresh and re-reads the diff. This is structural — workflow
scripts have no way to continue an existing agent — so the fix is *fewer rounds*, not better
ones. `MAX_FIX_ROUNDS = 6` with full re-derivation each time is the single largest line item.

**(c) The reconcile agent is doing amos's job, worse.** `milestone-loop.js:92–132` asks one
*sonnet* agent to do eight things in one prompt: read state, meter the day, fetch and
fast-forward, GC worktrees, enumerate candidates with blocked-by analysis, compute stack bases
by walking PR base chains, diff a comment-id ledger, and compute a delta probe. It is the only
place candidate selection can silently go wrong, it is the least verifiable spawn in the
system, and roughly two-thirds of it is a DAG query that a compiled binary already answers
deterministically for free.

Add `resilientAgent`: every schema exhaustion silently doubles that call.

---

## 3. Why it struggles to correct mistakes

Four root causes, only one of which is about model quality.

**No cross-attempt memory.** `attempt` is counted and capped at 3, then escalated. But attempt
2 restarts at Rederive knowing nothing about attempt 1 — what was tried, what failed, which
approach was ruled out. That history exists only in the run-log's `verdicts` field and whatever
prose got posted to the issue. The system is structurally incapable of learning from its own
failed attempt.

**The verdict is a vote-count, not a judgment.** On round 1, `runVerify` fans in
change-verifier + N zone lenses + craftsmanship + evidence-verifier and folds their severities
with an OR. Nobody adjudicates. Two lenses can raise contradictory findings and the implementer
receives both. Adjudication *does* exist on fix rounds (the `responseBlock` at lines 452–460 is
good design) — but by then three agents have already been spent.

This is precisely what regressed from `amos-next`. That skill's Step 7 made the reviewer's
verdict **advisory** and required the driving agent to read every flagged location itself and
form the final call. One accountable reader. The loop replaced that with a voting fan-in, which
produces more findings and less judgment. This is, I think, what you meant by "it doesn't self
perform reviews like Amos did" — the loop reviews *more*, but nobody decides.

**Classification is never corrected against reality.** `expertsForZones(zones)` at verify time
(`worktree-work.js:511`) uses the classifier's zones — derived from issue *prose*, before a
line of code existed. `composeStages` is recomputed after Rederive, but only for `touches_apple`
and `needs_bench`; zones are never revisited. A ticket classified `docs` that turns out to
rewrite the RHI gets no GPU lens, and there is no path in the system to notice.

**Everything actionable routes back to a fresh implementer, nits included.** Combined with (a),
this is why correction is slow: the loop's response to "you got a name wrong" is the same
machinery as its response to "this is incorrect."

---

## 4. What is genuinely worth keeping

Not much of this should be thrown away. Ranked by how load-bearing it is:

1. **Labels are display output; the router classifies fresh from content every pass.**
   (`rules/flow.md`, `CLAUDE.md`, and enforced in both classifier prompts: "labels are display
   output and must never be read as control input.") **This is already the thing you want to
   port to amos.** It exists, it works, and it is the direct answer to amos's staleness problem.
2. **Rederive.** "The issue body is the goal, not a spec" — verify every claim against current
   code before planning. Inherited from `amos-next` Step 2.5 and still the highest-value stage.
3. **Structural claim detection.** `claimed` is decided only by an open PR, a branch on origin,
   or an existing worktree — never by prose. `milestone-loop.js:116–119` explains why: "an issue
   comment saying work is queued is stale the moment it is written." Correct, and it means the
   ticket state map is a cache of something already derivable.
4. **Composed stage lists.** A bug ticket lands a failing test first; a docs ticket runs neither
   ABI nor bench. Right idea — but it's hardcoded control flow that should be a table.
5. **Pure-JS batch planning.** No agent judgment decides concurrency. Hot-file collision keyed
   per base branch, rig serialization, shallowest-first stack ordering. Keep verbatim.
6. **Domain experts + symptom indexes.** Non-derivable knowledge with an explicit
   "re-derive from code, never cache here" discipline. The best artifact in the repo.
7. **Independent-angle research.** `run-research.js` assigns one angle per investigator and
   forbids reading the issue's own recommendation, so conclusions stay independent. Small and
   good.
8. **Gating-lens tracking.** Only lenses that actually raised a gating finding re-run, and they
   re-check *their own* findings. Genuinely well-reasoned; keep.

---

## 5. What amos gives you for free

Everything the reconcile agent computes with tokens, amos computes with one GraphQL call:

- the ready set, milestone-scoped — `amos next --json` after `amos focus <milestone>`
- the blocked set with blockers named — `amos blocked --json`
- milestone inventory and counts — `amos milestones --json`
- DAG integrity (cycles, missing deps) — `amos validate`
- full issue body — `amos show --json`

Two properties matter beyond speed. First, **edges live in GitHub natively** — read from
`blockedBy`/`blocking`/`parent`/`subIssues` — so there is no local edge state that can drift.
Second, **status is adapter-derived**. The old `.amos-status` file that duplicated GitHub state
is already gone (`plan/33-adapter-driven-status.md`); don't re-solve that.

**amos's actual label problem is narrower than it looks.** It's one step in one skill:
`amos-next` Step 2 reads `.claude/workflows/<label>.md` for each GitHub label and treats those
files as mandatory. That's the whole coupling. Delete that step, adopt the loop's classifier,
and the staleness problem is gone. amos's *data model* never depended on labels — `labels` is an
optional display attribute in the spec.

**One real gap:** `amos graph` ignores `--json` (`src/main.rs:169–172` — ASCII only, returns
before the JSON branch). The `--json` doc comment on the CLI confirms it: "Works with
`milestones`, `next`, `blocked`, `orphans`, `focus`, `validate`, and `show`." An HTML graph
artifact needs edges as data, so this is the one upstream change.

---

## 6. The thesis

> **amos is the reconciler. The classifier is the router. The state file mostly disappears.**

The loop's most expensive and least verifiable agent is exactly the query amos was built to
answer. amos's only real weakness — routing on labels — is exactly what the loop's classifier
solves. They fit together with almost no adapter code, and the merge *deletes* far more than it
adds.

Concretely, replacing reconcile with amos removes:

- the eight-job reconcile prompt → `amos next --json` + `gh pr list --json` for stack bases
  (mechanical, no agent)
- the `tickets` map → claim state is already defined as purely structural; derive it
- `delta_probe` → hash `amos next --json` plus open-PR head SHAs
- `comment_ids` ledger and parked-question detection → gone with GitHub-as-mailbox (§8)
- `loop-status` skill → the artifact (§9)

---

## 7. Proposed architecture

Four layers, down from the current seven-ish.

### Layer 0 — amos, unchanged except `graph --json`

Reconciliation. `amos focus`, `next`, `blocked`, `graph`, `show`, all `--json`.

### Layer 1 — one skill

`/work-on <milestone or #N>`. Replaces `work-on-milestone`, `milestone-loop`, `work-on-ticket`,
and `loop-status`. It does only the four things a workflow script structurally cannot: touch
the filesystem, compute the date, use the sandbox bypass for the rig probe, and reach the owner.
Plus: run amos, run `gh pr list --json`, launch the workflow, render the artifact.

The milestone/ticket distinction becomes an argument, not two skills — `work-on-ticket` is
already documented as "the same workflow, same stages, same verifier" with one difference
(questions inline vs. parked), and once GitHub-as-mailbox is gone that difference evaporates.

### Layer 2 — one workflow file, two data tables

Collapse `milestone-loop.js` + `worktree-work.js` + `draft-design.js` + `run-research.js` into
one script. The dispatch `workflow()` call exists only to route on shape; with `pipeline()` the
per-ticket stage walk inlines directly, and the one-level nesting limit stops mattering.

Everything currently expressed as control flow becomes data:

```
ZONES = [ { zone, paths: [globs], expert, extraStages, gates } ]
SHAPES = { implement, 'bug-reproduce-first', 'design-first', research }
```

That one `ZONES` table drives expert selection, stage composition, *and* path→zone
re-derivation (§8). It also kills `KEEP-IN-SYNC(zone-router)`, which is currently duplicated
across three files.

`draft-design` and `run-research` become shapes with short stage lists, not separate scripts —
each is ~2 real stages wrapped in ~80 lines of copy-pasted `resilientAgent`.

**Concurrency is unaffected by the collapse.** Child workflows already share the parent's
concurrency cap (`min(16, cores−2)`), so inlining changes no scheduling behavior. The one thing
you give up is resuming a single ticket's sub-workflow by its own runId — minor, since
`resumeFromRunId` operates on the whole script anyway and caches the unchanged prefix.

### Layer 3 — the HTML artifact as the review surface

Regenerated at the end of every pass. Contains:

- the milestone DAG from `amos graph --json`, colored by live state (ready / in-flight /
  PR-open / blocked / parked)
- per node: derived zones, composed stage list, current position
- the plan-of-record, the review findings with their disposition, the diff stat
- a **"needs you"** section pinned at the top

This single surface replaces: `loop-status`, claim comments, plan-of-record comments,
design-brief comments, research-report comments, decision comments, the `gate` label, the
`comment_ids` ledger, and all parked-question detection.

---

## 8. Deriving work areas instead of reading labels

The loop already derives zones — but only once, from prose, and never corrects them (§3). The
upgrade is to make it **two-phase**, using the same `ZONES` table both times:

- **Phase 1, body-derived.** One sonnet classifier reads the issue against current code and
  emits shape + zones + rig needs. Used for routing, stage composition, and batch planning.
  Cached in the amos plan file so a re-run doesn't re-pay it.
- **Phase 2, path-derived.** After Implement, map the actual changed paths through `ZONES.paths`.
  Deterministic, free, and *correct* — it catches the docs-ticket-that-rewrote-the-RHI case that
  the current design cannot. Union the two sets before Verify picks lenses.

Re-derive **after Implement, before Gates** — not at Verify — so a late-added lens doesn't cost
an extra round.

This is strictly better than labels on all three axes that matter: it can't go stale (recomputed
every pass), it can't be wrong about what the code touches (phase 2 reads the diff), and it needs
no human maintenance.

**Cross-attempt memory** goes in the same place. On a failed attempt or an escalation, write the
attempt history — what was tried, what failed, what was ruled out — into the amos plan file body.
The next Rederive reads it via `amos show --json`. This is amos's existing "the issue has the
*what*, the local markdown has the *how-for-AI*" mechanism, currently unused by the loop, and it
is the direct fix for the correction problem. It's also local-only, so nothing reaches GitHub.

---

## 9. Killing GitHub-as-mailbox

The loop writes to GitHub issues in at least six places: the claim comment, the plan-of-record,
design briefs, research reports, propose-only plans, and `## ⛔ DECISION NEEDED` blocks with a
`gate` label. The `comment_ids` ledger exists *only* because the loop's `gh` identity shares the
owner's login, so it can't recognize its own comments.

Under the proposal GitHub keeps exactly two roles: **source of truth** for issues, edges, and
milestones (read by amos), and **destination for PRs**. Nothing else is written.

Everything narrative goes to the artifact. Questions go through `AskUserQuestion`.

**The honest trade-off:** the current design posts to GitHub so that a *headless* firing can
still make progress on a question. Remove that and an unattended pass must park and stop rather
than ask. I think that's clearly the right trade here, for a reason the loop's own docs already
concede: **the heartbeat is session-scoped and fires only while the REPL is idle.**
`CronCreate` jobs write nothing to disk, the `durable` flag has no effect, jobs expire after
7 days, and closing the session stops the loop. There is no true unattended mode — so
GitHub-as-mailbox was solving a problem that mostly doesn't exist. Name the trade explicitly in
the PR rather than letting it slip in.

---

## 10. Fix-round economics

Three changes, which together should take a typical ticket from ~26 spawns to ~12–14:

**Add one adjudicator.** Between the review fan-in and any fix round, a single opus agent reads
the findings *and the diff* and assigns a disposition per finding: fix-now / fix-as-nit /
decline-with-reason / owner-question. It must cite the diff for anything it drops. This restores
`amos-next`'s single accountable reader without giving up the fan-out's coverage. One extra
spawn that can eliminate two to four rounds — net saving *and* better judgment.

**Nits stop forcing rounds.** `low` findings are collected and applied in the final fix pass,
self-attested, never re-verified. Only `blocker` and `should-fix` trigger a re-verify. The stated
goal — a human never spends attention on a nit — is preserved; the cost isn't.

**`MAX_FIX_ROUNDS`: 6 → 3.** Six rounds with full re-derivation each time is the largest single
line item, and given (b) in §2, round 5 is not meaningfully better informed than round 2. With
the adjudicator in front, three is generous.

Keep gating-lens tracking and delta re-verify exactly as they are — they're already right.

---

## 11. Scope: one issue, one or two PRs

`rules/flow.md` already requires operating-model changes to land as their own PR with rationale,
so this is a natural fit.

**streamlib PR** — the substance:
- add the single workflow file + `ZONES` / `SHAPES` tables
- add the artifact renderer
- collapse four skills into one
- rewrite `.claude/loops/README.md` knobs and delete the ticket-map / ledger / delta-probe state
  schema
- delete `milestone-loop.js`, `worktree-work.js`, `draft-design.js`, `run-research.js`,
  `loop-status/`, `work-on-ticket/`, `milestone-loop/`

**amos PR** — `graph --json`. Roughly a 40-line change: reuse the existing `node_to_json`
serializer and emit edges alongside nodes.

**Can it be one PR?** Only if the artifact is built from `next` + `blocked` + `milestones`
`--json`, which already exist. That works if `blocked --json` carries enough blocker/edge data
to reconstruct the graph — worth checking before committing to the one-PR path, because a graph
needs edges and the ready set alone doesn't have them. `graph --json` is the clean answer and
it's small; I'd rather spend the 40 lines than reconstruct edges from two query outputs.

**Explicitly out of scope for this PR:** the domain experts and symptom indexes (they're fine,
don't touch them), the hooks, the rules files, and rig/evidence handling (it moves into the
`ZONES` table as data but its behavior shouldn't change in the same PR that restructures
everything else).

---

## 12. Corrections and gotchas

**Workflow scripts are plain JavaScript, not TypeScript.** Type annotations, interfaces, and
generics fail to parse at load. Two options: author `.ts` and build to `.js` with esbuild
(commit both, point `scriptPath` at the `.js`), or write `.js` with `// @ts-check` and JSDoc
`@typedef` — full editor typechecking, zero build step. For a single file, the second; a build
step for one file is the kind of over-engineering you're trying to leave behind.

**`Date.now()`, `Math.random()`, and argless `new Date()` throw inside workflow scripts.** Hence
`today` arriving as an argument. This constraint survives the redesign — keep the date in the
skill.

**Workflow scripts have no filesystem access.** State reads/writes, the kill switch, and the
artifact write all stay in the skill.

**`workflow()` nests one level only.** Irrelevant after the collapse, but it's why the current
split exists — don't reintroduce it by accident.

---

## 13. Risks worth validating first

1. **The adjudicator becomes a rubber stamp.** Mitigation: require a diff citation for every
   finding it drops, and log dropped-vs-kept counts to the run-log so drift is visible.
2. **Path→zone re-derivation adds a lens late and costs a round.** Mitigation: re-derive after
   Implement, before Gates — never at Verify.
3. **Deleting the ticket map removes the fallback for claim detection.** The stated rule is
   already "structural evidence only," but verify that `gh pr list` + `git worktree list` +
   `git ls-remote` genuinely covers every case before dropping the map.
4. **Losing GitHub comments loses the durable audit trail.** The artifact is regenerated, not
   append-only. Keep `run-log.jsonl` as the durable record and treat the artifact strictly as a
   view over it.
5. **One file gets long.** The current four total ~1,500 lines; the collapse should land near
   400–500 once the four `resilientAgent` copies and the three zone routers become one each.
   If it doesn't, the tables aren't absorbing enough control flow — that's the signal to check.

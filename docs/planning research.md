# Writing Implementation Plans for Agentic Execution with Claude Code: A Practitioner's Guide

## TL;DR

- **Write plans as ~200-line scannable Markdown documents that pair explicit file paths and small code/pseudocode snippets with stated *intent*, phased verification checkpoints, and an explicit "Not Doing" section** — the structure converged on by HumanLayer, GitHub Spec Kit, Addy Osmani, and Anthropic's Claude Code team. Plans below this floor under-specify; plans much above it lose the human reviewer and bloat the agent's context window, which experienced users recommend keeping under ~40–60% of capacity.
- **Match detail level to risk and novelty, not to scope.** Single-file feature in a familiar area: skip the plan ("if you could describe the diff in one sentence, skip the plan" — Anthropic's Claude Code docs). Multi-file feature, unfamiliar code, or destructive change: write a full phased plan with automated + manual verification gates. Greenfield project: write a hierarchy (constitution → PRD/spec → per-feature plans) rather than one mega-doc.
- **The optimal level of prescription is "tight on WHAT, loose on HOW."** Specify file paths, public interfaces, data shapes, acceptance criteria, and non-goals precisely; let the agent choose internal implementation details, helper-function names, and ordering within a phase. Boris Cherny (Claude Code's creator) and Addy Osmani both report this is where the one-shot success rate goes from roughly one-third (Anthropic's reported autonomous first-attempt rate) to near-deterministic.

## Key Findings

1. **A converged plan template exists** across HumanLayer, GitHub Spec Kit, Anthropic, Addy Osmani's agent-skills repo, and the BMAD framework. The shared spine is: Overview/Why → Current State → Desired End State → Out-of-Scope → Phased Changes (file-by-file with snippets) → Per-phase Success Criteria (automated + manual).

2. **The single most expensive mistake is letting the agent code before the plan is agreed.** Boris Cherny, quoted by Gergely Orosz in The Pragmatic Engineer (March 2026): *"once there is a good plan, it will one-shot the implementation almost every time."* The Claude Code team designed Plan Mode (Shift+Tab twice) specifically to enforce this.

3. **Plan length is a proxy for review quality, not for prescription density.** Dex Horthy (HumanLayer), in the ACE-FCA write-up: *"I can't read 2000 lines of golang daily. But I can read 200 lines of a well-written implementation plan."* The 200-line target is the human-reviewability ceiling, not a coding budget.

4. **Code snippets in plans are intent anchors, not literal copy-paste.** HumanLayer's `create_plan.md` template explicitly embeds ```[language] // Specific code to add/modify``` blocks, but the matching `implement_plan.md` command tells the executor agent: *"Follow the plan's intent while adapting to what you find … The plan is your guide, but your judgment matters too."*

5. **CLAUDE.md and plan documents serve different jobs.** CLAUDE.md is durable, project-wide context (conventions, commands, gotchas) — HumanLayer keeps theirs under 60 lines, the community consensus is under 300. Plan documents are ephemeral, task-scoped artifacts that live in `thoughts/shared/plans/` or `specs/<feature>/`.

6. **Verification gates are the load-bearing element**, not the prose. Plans without per-phase, programmatically checkable success criteria degrade into "looks good" rubber stamps. The HumanLayer template formalizes this with explicit Automated Verification (e.g., `make test`) and Manual Verification checkboxes per phase.

7. **Hierarchical plans beat monolithic plans for anything bigger than a single feature.** GitHub Spec Kit enforces this structurally: `constitution.md` (immutable principles) → `spec.md` (WHAT/WHY) → `plan.md` (HOW) → `tasks.md` (atomic units) → `research.md` + `contracts/` + `data-model.md` for design artifacts.

8. **Common failure modes are well-documented and almost all are prevented by plan structure**: scope creep (fixed by "What We're NOT Doing"), context degradation (fixed by phased compaction), sycophantic agreement (fixed by adversarial review subagents), and silent success-without-verification (fixed by automated checkpoints).

---

## Details

### 1. The Canonical Plan Structure

After comparing HumanLayer's open-source `create_plan.md`, GitHub's Spec Kit `plan-template.md`, Addy Osmani's `agent-skills/.claude/commands/plan.md`, BMAD, and Anthropic's own guidance, the convergent template is:

```markdown
# [Feature/Task Name] Implementation Plan

## Overview
[2–4 sentences: what we're implementing and why. Link to ticket/PRD.]

## Current State Analysis
[What exists now, with file:line references discovered in the Research phase.
Key constraints, existing patterns to follow.]

## Desired End State
[Specification of the final state and how to verify it externally —
e.g., "POST /api/oauth/callback returns 200 with a valid session cookie
when given a Google ID token."]

### Key Discoveries
- `src/auth/session.go:142` — existing session creation; reuse
- `src/middleware/csrf.go` — must wrap new endpoint
- Constraint: cannot add new runtime dependencies (per CLAUDE.md §Deps)

## What We're NOT Doing
- No refactor of the existing email/password flow
- No support for refresh tokens in v1
- No UI changes (frontend ticket FE-412 tracks that)

## Implementation Approach
[2–3 paragraphs of high-level strategy. Explain non-obvious choices
and rejected alternatives. This is the reviewable architecture.]

## Phase 1: [Descriptive Name, e.g., "Provider abstraction"]
### Overview
[One paragraph: what this phase accomplishes and why it's first.]

### Changes Required

#### 1. New file: `src/auth/oauth/google.go`
**Changes**: Implement `GoogleProvider` satisfying the existing `Provider` interface.
```go
type GoogleProvider struct {
    clientID     string
    clientSecret string
    httpClient   *http.Client
}

func (g *GoogleProvider) ExchangeCode(ctx context.Context, code string) (*Identity, error) {
    // ...
}
```

#### 2. Modify: `src/auth/registry.go`
**Changes**: Register `GoogleProvider` in the provider map.
[Sketch the diff at the call-site, not full file.]

### Success Criteria

#### Automated Verification
- [ ] `make test ./src/auth/oauth/...` passes
- [ ] `go vet ./...` clean
- [ ] New endpoint returns 200: `curl -X POST localhost:8080/api/oauth/google/callback ...`

#### Manual Verification
- [ ] End-to-end flow works against real Google account in staging
- [ ] Error path (revoked token) renders friendly error in UI

## Phase 2: [Next phase] ...
```

This template is reproduced almost verbatim across HumanLayer's repo, addyosmani/agent-skills, and (with cosmetic differences) GitHub Spec Kit. Treat it as the load-bearing skeleton.

### 2. How Detailed Each Section Should Be

This is the headline question. The evidence is unusually consistent across practitioners:

**Overview / Why** — *3–5 sentences max.* Enough that a future engineer (or a fresh Claude session after a context compaction) can re-establish intent without re-reading the ticket.

**Current State** — *As detailed as needed to be unambiguous; use file:line citations, not prose summaries.* HumanLayer's plans look like `src/payments/charge.go:88 — current Stripe call site` because the agent can grep for that and the human can click it. Vague "the payments module handles charges" is worse than useless: it inflates the document without anchoring it.

**Desired End State** — *Externally observable behavior, not internal architecture.* "After this plan, posting to `/api/refund` with a valid order_id returns 200 and emits a `refund.succeeded` event on the queue." If you can't write this as a black-box description, you don't yet understand the task.

**What We're NOT Doing** — *Enumerate aggressively.* This is the single highest-ROI section. Without it, Claude will routinely "helpfully" rewrite an adjacent module. GitHub Spec Kit's templates explicitly forbid speculative or "might-need" features and require every feature to trace back to a concrete user story with acceptance criteria.

**Implementation Approach** — *Long enough to justify rejected alternatives.* 2–4 paragraphs. This is what a senior reviewer reads to decide whether the plan is sane. If you can't articulate why approach A over B, you haven't done the research phase.

**Per-phase Changes** — *Specify file paths and public interfaces precisely; sketch — don't write — internal code.* The HumanLayer template includes language-tagged code blocks, but the matching `implement_plan.md` command explicitly instructs the executor: *"Follow the plan's intent while adapting to what you find … The plan is your guide, but your judgment matters too."* The snippet is an intent anchor, not literal copy-paste.

**Success Criteria** — *Always programmatically checkable where possible.* `make test` and `curl ... | jq .status` beat "the endpoint works correctly." The HumanLayer convention splits these into Automated (the agent runs them) and Manual (a human confirms them) checkboxes, which is the cleanest pattern in the wild.

### 3. The Pseudocode / Exact-Code Decision

The community is divided in form but unified in principle. HumanLayer embeds code snippets. JetBrains Junie supports "strategy-only" plans (*"If you prefer a strategy-only document, phrase your prompt explicitly: 'Generate a plan focusing only on what needs to be done, without code examples.'"*). Alejandro Piad Morffis ("AI Coding Agents, Deconstructed") writes that plans should describe *"what files must be touched and what must be done in there (semantically, not code)."*

The synthesis that holds across all of them:

- **Include snippets when**: the call-site, type signature, or data shape is the actual locus of decision (e.g., a new interface, a SQL migration, an API contract).
- **Omit code when**: it's straightforward implementation that the agent can derive from the surrounding codebase (e.g., a CRUD handler that mirrors three existing ones, or a unit test layout that follows project convention).
- **Never include full function bodies.** That's the agent's job. A full function body in the plan is a signal you're using the plan as a place to write code rather than to think.

A useful rule of thumb from JetBrains Junie: *"The tasks should be specific enough that you can mark them as complete once finished, but not so granular that they devolve into meaningless busywork. Think 'implement new data repository' rather than 'write a function.'"*

### 4. Scope-Specific Guidance

| Scope | Plan Format | Length | Key Sections |
|---|---|---|---|
| Single small feature, familiar code | Inline plan from Plan Mode, no file | "If you could describe the diff in one sentence, skip the plan" (Anthropic) | None — direct prompt |
| Single feature, multi-file or unfamiliar | One `plans/YYYY-MM-DD-feature.md` | ~150–300 lines | Full template above, 1–3 phases |
| Module / multi-feature | One spec + one plan, possibly per sub-feature | Spec ~200 lines + plan ~300 lines | Constitution reference, contracts/ subdirectory, explicit phase boundaries |
| Greenfield project | Hierarchical: constitution → PRD → per-feature plans | Constitution: ~60 lines. PRD: ~300 lines. Per-feature: ~200 lines each | Use GitHub Spec Kit or BMAD scaffold |

For greenfield work, the strong recommendation is to use a hierarchical artifact tree. GitHub Spec Kit's structure (`memory/constitution.md`, `specs/[###-feature]/{spec,plan,research,data-model,quickstart,tasks}.md`, optional `contracts/`) is the most polished open-source version and is now agent-agnostic — Claude Code has a first-class integration.

The reason hierarchy beats monolithic for greenfield: a single 2000-line doc exceeds reviewable size for humans *and* eats too much of the agent's context window. Splitting along Spec Kit lines means the agent loads only the relevant slice per task while the constitution (immutable principles like "no new runtime deps without ADR", "all migrations reversible") is always pinned.

### 5. Risk-Based Detail Calibration

Risk level should dial detail up or down independently of scope:

- **Low risk** (additive, behind a feature flag, in a sandboxed module): looser plans, more agent autonomy, automated verification only.
- **Medium risk** (touches shared code, mutates data, changes a public API): full template with manual verification gate per phase.
- **High risk** (auth, payments, migrations, destructive ops): plan must include rollback steps, explicit "Manual Verification" gates that block automated continuation, and ideally a separate adversarial reviewer pass before execution. Boris Cherny: one team has a second Claude review the plan *"as a Staff Engineer"* before execution; if things go sideways mid-implementation, *"re-plan from scratch, don't patch."*

Anthropic's own report "How Anthropic teams use Claude Code" notes — speaking specifically about the RL Engineering team — that *"they acknowledge it only works on first attempt about one-third of the time, requiring either additional guidance or manual intervention."* That number is the empirical justification for verification gates: roughly two-thirds of autonomous attempts need re-steering, and the gates are what surface the misstep before it propagates.

### 6. Claude Code-Specific Best Practices

**Plan Mode.** Toggle with Shift+Tab twice; activates a read-only state where Claude has access to Read, Glob, Grep, WebFetch, and the Task subagent, but no Edit, Write, or Bash. The Claude Code team's recommended four-phase workflow is: **Explore → Plan → Implement → Commit.** Anthropic's official guidance:

> "Plan Mode is useful, but also adds overhead. For tasks where the scope is clear and the fix is small (like fixing a typo, adding a log line, or renaming a variable) ask Claude to do it directly. Planning is most useful when you're uncertain about the approach, when the change modifies multiple files, or when you're unfamiliar with the code being modified. If you could describe the diff in one sentence, skip the plan."

And on prompt precision:

> "The more precise your instructions, the fewer corrections you'll need. Claude can infer intent, but it can't read your mind. Reference specific files, mention constraints, and point to example patterns. Vague prompts can be useful when you're exploring and can afford to course-correct."

**Where plans live.** Plan Mode output defaults to `~/.claude/plans/` (user-level). For team workflows, override with `.claude/settings.json`:

```json
{ "plansDirectory": ".claude/plans" }
```

HumanLayer commits plans to `thoughts/shared/plans/YYYY-MM-DD-ENG-XXXX-description.md` — versioned alongside code, reviewable in PRs, and discoverable by future sessions. Strongly recommended.

**CLAUDE.md vs. plan.md.** These have distinct jobs and should not be conflated:

- `CLAUDE.md` (project-root): durable, loaded every session. Conventions, commands, gotchas, pointers to docs. HumanLayer keeps theirs to ~60 lines; Anthropic's own internal CLAUDE.md is ~2.5k tokens (~100 lines, per Boris Cherny's reporting). The community ceiling is ~300 lines — beyond that, important rules get lost in noise.
- `plan.md` (per-task): ephemeral, loaded only when working on that task. The full structured template above.

The TurboDocx WHY/WHAT/HOW pattern is a useful CLAUDE.md spine: WHAT (one-line project description + tech stack), HOW (essential commands), structure map, conventions (only non-default patterns), and progressive-disclosure pointers to deeper docs using `@docs/api-architecture.md` syntax.

**Subagents.** Use them to keep the main context clean. Adam Wolf (Claude Code core engineer, paraphrased by community): *"Sub agents work best when they just look for information and provide a small amount of summary back to main conversation thread."* Three useful patterns:
- **Research subagent** — loads the codebase into its own 200K window, returns a 200-line summary. Built-in Explore subagent (Haiku-powered) auto-activates in Plan Mode for exactly this.
- **Reviewer subagent** — fresh context, no implementation history, evaluates the plan or PR adversarially. Boris Cherny's code-review command spawns five at once, each finding issues, then five more poking holes in their findings.
- **Implementer subagent** — only when the work is truly parallelizable. Do not use subagents to implement coupled changes; the lack of mid-task communication causes drift.

Project subagents live in `.claude/agents/<name>.md` with YAML frontmatter (name, description, tools, model). Tools should be scoped tightly — a reviewer needs Read/Grep/Glob, never Write.

**Slash commands.** For workflows you run repeatedly, store them in `.claude/commands/` and check into git. HumanLayer's open-source `create_plan.md`, `implement_plan.md`, and `research_codebase.md` commands are an excellent starting point and worth cloning verbatim.

**Hooks for guardrails.** PostToolUse hooks (formatters, linters, type-checkers) catch the "last 10%" where Claude's output is almost-but-not-quite right. Stop hooks can keep a session running until tests pass — `/goal all tests in test/auth pass and the lint step is clean` won't stop until the condition is true.

**Context budget.** Empirical community consensus, corroborated by HumanLayer's case studies on a 300k-LOC Rust codebase: quality begins degrading at 40–60% context utilization. Start fresh sessions per task. If you've corrected Claude more than twice on the same issue, the context is polluted with failed approaches — `/clear` and restart.

### 7. General Agentic Execution Best Practices (Tool-Agnostic)

The Research → Plan → Implement workflow popularized by Dex Horthy at HumanLayer is the dominant pattern across tools (Claude Code, Cursor, OpenAI Codex, Cline). Key principles:

1. **Spec is the source of truth, code is the regenerable output.** GitHub's Spec Kit, AWS Kiro, and Tessl all converge on this. The test: *"could an agent rebuild this feature from this spec alone and produce behaviorally identical output?"* (Augment Code's "regeneration test").

2. **Use EARS notation for acceptance criteria** (Easy Approach to Requirements Syntax — Ubiquitous, Event-driven, State-driven, Unwanted-behavior, Optional-feature). Every major SDD tool uses EARS because the structure is parseable by both humans and LLMs.

3. **Slice vertically, not horizontally.** Each phase should be one complete end-to-end path that can be merged independently, not "all the backend first, then all the frontend." This is addyosmani/agent-skills' explicit guidance and matches HumanLayer's phase examples.

4. **Verification is an external oracle.** Without tests, Claude's only check is its own judgment, which degrades with context length. TDD is the single strongest pattern in agentic coding. Anthropic's recommended sequence: write tests first ("no mock implementations"), then implement.

5. **Acceptance-driven backpressure.** Derive required tests from acceptance criteria *during* planning. Connect spec → required tests → implementation. Prevents the agent from claiming "done" without passing the tests that prove it.

6. **Common failure modes are predictable** (six classic ones per MindStudio's "AI Agent Failure Pattern Recognition" article): context degradation, specification drift, sycophantic confirmation, tool-call failures, cascading failure, silent failure. All six are mitigated by the same plan structure: explicit phases, automated checkpoints, fresh-context reviewers, and external verification.

### 8. Hierarchical Plans for Larger Work

When a single plan would exceed ~300 lines, hierarchize. The recommended structure for a multi-feature module or greenfield project:

```
.specify/memory/constitution.md       # 50–100 lines — non-negotiable principles
docs/PRD.md                            # 200–400 lines — product spec (WHAT, WHY)
specs/001-auth/
  spec.md                              # feature-level WHAT (~200 lines)
  research.md                          # codebase findings, alternatives
  plan.md                              # technical HOW (~200–300 lines)
  data-model.md                        # entities and relationships
  contracts/openapi.yaml               # API contracts
  quickstart.md                        # validation walkthrough
  tasks.md                             # atomic, ordered task list
specs/002-billing/...
CLAUDE.md                              # session-load context (≤300 lines)
```

GitHub Spec Kit auto-generates this scaffold (`uvx --from git+https://github.com/github/spec-kit.git specify init my-project`) and is the most refined open-source implementation. For Claude Code users specifically, the `cc-sdd` skill installation gives you the slash commands `/speckit.specify`, `/speckit.plan`, `/speckit.tasks`, `/speckit.implement` for the full pipeline.

The constitution (memory/constitution.md) is non-obvious but very high-leverage. It encodes immutable architectural rules — "No new runtime dependencies without an ADR", "All migrations must be reversible", "Tests must precede implementation" — that every spec and plan must respect. Without it, the agent will quietly drift from your team's standards over many sessions.

### 9. The Autonomy ↔ Prescription Trade-off

This is where teams burn the most time getting it wrong. Matthew Diakonov's "The Paradox of Autonomy – Constraints Make AI Agents Useful" (Fazm Blog, March 18, 2026) frames the tension cleanly:

> "More autonomy should mean more capability. … In practice, the opposite happens. An agent with full autonomy faces an enormous decision space. … Unconstrained agents make confident choices that seem reasonable locally but waste time globally. They refactor code that did not need refactoring. They investigate errors that resolve themselves. They optimize things that are not bottlenecks. A daily task list is not a limitation — it is a capability amplifier."

Augment Code's writeup distills Anthropic's own context-engineering principle: *"strive for the minimal set of information that fully outlines expected behavior."* Over-specification causes its own failure modes — misdiagnosis, brute-force fixes, and the agent following the letter of the plan while missing its intent.

The practical rule that emerges:

- **Tight on WHAT**: file paths, public interfaces, data shapes, acceptance criteria, non-goals, conventions.
- **Loose on HOW**: internal implementation details, helper-function naming, ordering within a phase, choice of standard-library idioms.

Addy Osmani, in "The 80% Problem in Agentic Coding" (addyo.substack.com), summarizes this in one line: *"Spend 70% of effort on problem definition, 30% on execution. Write comprehensive specs, define success criteria, provide test cases up front. Guide the agent's goals, not its methods."*

### 10. Notable Open-Source Plans to Learn From

- **humanlayer/humanlayer** (`.claude/commands/create_plan.md`, `implement_plan.md`, `research_codebase.md`) — the most copied production-grade plan template in the community. Treat as canonical.
- **github/spec-kit** (`templates/plan-template.md`, `templates/spec-template.md`) — the most rigorous multi-document SDD template; agent-agnostic.
- **addyosmani/agent-skills** (`.claude/commands/plan.md`) — concise reference implementation with explicit vertical-slicing guidance.
- **shanraisshan/claude-code-best-practice** — a curated atlas of community patterns and a working orchestration-workflow example.
- **anthropics/claude-code** (`docs/`) — the canonical workflow reference.

---

## Recommendations

**For your immediate next plan (single feature, multi-file)**:

1. Start in Plan Mode (Shift+Tab twice). Don't write code until the plan is approved.
2. Use the HumanLayer template skeleton, committed to `thoughts/plans/YYYY-MM-DD-<feature>.md` or `.claude/plans/<feature>.md`.
3. Target ~200 lines. If you exceed 300, split into phases or sub-plans.
4. Include code snippets only at decision-locus interfaces (new types, contracts, migration shapes). Never paste full function bodies.
5. Every phase must have both Automated and Manual verification checkboxes. No "looks good" gates.
6. Include an explicit "What We're NOT Doing" section. This is the single highest-ROI bullet.
7. Before executing, spawn a reviewer subagent: *"As a staff engineer, find three flaws in this plan."* Address them, then proceed.

**For a multi-feature module**:

8. Promote shared decisions to `CLAUDE.md` (≤300 lines, ideally ≤100). Keep per-feature plans short by referencing it.
9. If two features share data shapes or contracts, factor them into a `data-model.md` or `contracts/` directory referenced by both plans.

**For a greenfield project**:

10. Run `specify init` from GitHub Spec Kit, or equivalent BMAD scaffold.
11. Write the constitution *first*. 50–100 lines. Include immutable principles (test-first, no new deps without ADR, reversible migrations, etc.).
12. Write the PRD before any plan. Then one plan per feature, executed sequentially with merge points.
13. Update CLAUDE.md *as you discover what was missing*. Anthropic's internal rule: *"Anytime we see Claude do something incorrectly, we add it to CLAUDE.md so it doesn't repeat next time."*

**Thresholds that should change your approach**:

- **Plan exceeds 400 lines** → split into phases or sub-plans. You've crossed the human-reviewability ceiling.
- **CLAUDE.md exceeds 300 lines** → prune ruthlessly; convert any "Claude already does this correctly" rules to deletions, any style rules to hooks/linters.
- **You correct Claude more than twice on the same issue** → context is polluted; `/clear` and restart with an updated plan.
- **Context utilization > 60%** → either compact intentionally (preserving the plan path and current task) or start a fresh session.
- **First-shot success rate falls below ~50%** → your plans are under-specified on WHAT, or your verification gates are missing.
- **You're routinely deleting agent-generated code** → your plans are over-specified on HOW, or you're letting the agent code before the plan is approved.

## Caveats

- **The field moves fast.** Claude Code shipped major features (Plan Mode, subagents, skills, hooks, Agent Teams, checkpoints, web sessions) across 2025–2026. Anything in this guide tied to specific UI keystrokes or settings may shift; the structural principles (Research → Plan → Implement, vertical slicing, external verification, hierarchical artifacts) have been stable.
- **Most quantitative claims are practitioner self-reports, not controlled studies.** The "~200-line plan" target (Dex Horthy, ACE-FCA), the "40–60% context utilization" rule (HumanLayer case studies), the "about one-third" first-attempt success rate (Anthropic's RL Engineering team, per the official "How Anthropic teams use Claude Code" report), and the "20–30 PRs per day" claim (Boris Cherny, via Gergely Orosz's Pragmatic Engineer interview in March 2026) come from specific individuals and contexts and may not generalize. Treat them as starting calibration, not ground truth.
- **Spec-driven development has real overhead.** For exploratory prototypes, throwaway scripts, and one-shot fixes, full plan documents are counterproductive. The pattern multiple practitioners converge on: vibe-code the spike, distill the result into a spec, then spec-drive the production version.
- **Multi-agent and Agent Teams patterns are still experimental.** Anthropic's Agent Teams ships behind an env var (`CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`). For most teams in 2026, single-session-with-subagents is the right answer; parallel sessions with worktrees handle the rest.
- **CLAUDE.md instructions are not deterministic.** Even rules marked "IMPORTANT" or "YOU MUST" are sometimes ignored as conversations grow. Pair every prohibition with a positive direction ("Never use `--legacy-peer-deps`; resolve conflicts by updating the package to a compatible version") and enforce critical rules with hooks rather than prose.
- **The HumanLayer "200-line plan / 7-hour 35k-LOC feature" case study is a high-skill demonstration**, not a typical workflow. Per HumanLayer's blog post "Advanced Context Engineering for Coding Agents," Dex Horthy and BoundaryML co-founder Vaibhav Gupta (@hellovai) *"shipped 35k LOC of working BAML code in 7 hours"* — research/plan/implement is not autopilot; it's intensive steering of a very fast executor.
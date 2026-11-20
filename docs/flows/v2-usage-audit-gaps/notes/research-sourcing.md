Verdict: All four gaps are real. The source rules bind only agents: research-methods SKILL.md:34 is written around a pinned dependency, the investigator is told to use Context7 only to check that it is available (plan-investigator.md:22), and the worker's run-note parser rejects any `[[sources]]` key (run_note.rs:298). Nothing reaches orchestrator-written text (elaborate.md:39-47, write-and-prove.md:74, drain.md:402-419, plan-flow.md:249), and no store shares a fetched source between cells or tasks. Recommended design: one platform rung in research-methods, a source rule for orchestrator-written facts with a cheap tool check, a per-run source cache keyed by URL, and a parsed `[[sources]]` table in run notes that is promoted to a repo-level reference file.

```toml sources
read_at = "6b6c579c7af3863440ee59552c3f91cb9481b3cb"
dirty = []

[[source]]
path = "claude/skills/research-methods/SKILL.md"
line = 34
claim = "The source hierarchy starts at the lockfile and pinned dependency. It has no rung for a hosted platform or service, and no rule against a constructed URL"

[[source]]
path = "claude/skills/research-methods/references/web-doc-research.md"
line = 19
claim = "The Context7-first procedure and the llms.txt advice sit in a gated reference that is read only when a finding turns on a library or API"

[[source]]
path = "claude/agents/plan-investigator.md"
line = 22
claim = "The investigator body names Context7 only to say ToolSearch must run before reporting it unavailable. It never orders Context7 or a search before a fetch"

[[source]]
path = "claude/agents/plan-worker-deep.md"
line = 153
claim = "The worker is told outright: Context7 first, the web second, never memory. Workers do use Context7"

[[source]]
path = "claude/agents/plan-worker-lite.md"
line = 156
claim = "The lite worker carries the same rule: check Context7, then the web"

[[source]]
path = "claude/agents/review-finder.md"
line = 19
claim = "The finder only defers to research-methods. It has no line on when to look something up or on reusing a fetch"

[[source]]
path = "claude/agents/review-verifier.md"
line = 18
claim = "The verifier loads research-methods for evidence rules only. Nothing points it at pages the run already fetched"

[[source]]
path = "claude/skills/review-v2-method/SKILL.md"
line = 294
claim = "The review method defers the source hierarchy to research-methods unchanged"

[[source]]
path = "tomlctl/src/review/fields.rs"
line = 1725
claim = "The verifier projection withholds the finder's grade and fix on purpose, for independence. A shared page cache must not carry conclusions"

[[source]]
path = "claude/workflows/review-v2.js"
line = 252
claim = "Every cell's preamble already names the run directory, so a per-run source cache needs no new Workflow argument"

[[source]]
path = "claude/skills/plan-v2-engine/references/elaborate.md"
line = 39
claim = "Elaboration reads the defer reason, brief, notes and code, then writes specs. It has no step to source a new outside fact and does not launch an investigator"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 74
claim = "Handover rows carry reason, action, when, done_when and source_task. They have no source field and no rule that a console label be checked"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 402
claim = "The orchestrator writes handovers from worker proposals, with no source requirement"

[[source]]
path = "claude/skills/review-v2-engine/references/plan-flow.md"
line = 249
claim = "An advisory fixed by hand is recorded as addressed, with no check on any new outside value the hand edit introduces"

[[source]]
path = "tomlctl/src/plan/run_note.rs"
line = 298
claim = "The run-note block rejects an unknown key, so a worker that adds a sources table today makes its whole note malformed"

[[source]]
path = "claude/skills/plan-v2-engine/references/open-and-challenge.md"
line = 162
claim = "Dependency cards are investigator-built, but only for new third-party dependencies, not for platform values or image tags"
```

## What happened

**B-98d14bca: orchestrator-written platform claims**

- **partnerhub_pages, task 34 (site-worker-new-strapi).**
  - The orchestrator wrote the task's spec when it elaborated milestone C, in commit f1ab790 on 2026-11-20. The spec said to read cpuTime "the way notes/spike-workerd-render.md did (wrangler tail, JSON format, with the generated preview config), or through Workers observability".
  - The spike measured throwaway workers.dev Workers, where tail works. Task 34 measured a Worker Preview version of the production script, which tail does not see. So the source existed but did not cover the object the claim was about.
  - The round-1 worker ran in session b2a1b292, 2026-11-20 09:03-09:17 UTC. It found tail blind and the observability API answering 403 on both tokens. It fetched the observability API reference and switched to GraphQL `workersInvocationsAdaptive`.
  - It returned `failed`. The direct cause was that the preview answered 500 on every content route; the wrong tail instruction cost the extra research and a deviation.
  - The orchestrator rewrote the spec ("tail does not see the preview's invocations… read from the GraphQL Analytics API"), and round 2 (09:33-09:58 UTC) passed.
- **partnerhub H2 and H5.**
  - H2 was approved as a list of permission scopes (commit 03e4904). In session 36cbd3f6 at 2026-11-19 13:47 UTC, the user reported that the wording did not match the dashboard. The orchestrator then dispatched an agent and rewrote H2 with "dashboard labels" before milestone A was recorded (961580d).
  - H5's dashboard steps were taken from the repo's docs/deploy.md. In session b2a1b292 at 2026-11-20 10:58 UTC, the user reported that the settings did not match. Only then did the orchestrator fetch the current Workers Builds docs, and it found that deploy.md used stale labels and left out two settings.
  - In both cases the source lookup happened after the user hit the wrong text.
- **strapi-server F78 (cloud-deploy-parallel-run, run U2).**
  - The advisory said the lockfile was "maintained with Bun 1.4.5". The local Bun was a 1.4.5 canary build, and the store records it as 1.4.5-canary.1.
  - It was fixed by hand with the owner's agreement, pinning `oven/bun:1.4.5`. Nobody checked that the tag existed.
  - The milestone A gate failed on `docker build` ("failed to resolve source metadata … oven/bun:1.4.5: not found"), which led to fix task 31.

**B-6d3fb3e3: repeated fetches in one review run**

- Partnerhub run U2 ran in session 724defa2 on 2026-11-19, 12:09-12:30 UTC. Four cells fetched the same Cloudflare pages for one question:
  - finder C13
  - verifier of F65
  - finder C17
  - verifier of F82
- `workers/static-assets/headers` was fetched 4 times, `pages/configuration/serving-pages` 4 times and `pages/configuration/headers` twice: 10 fetches of 3 pages.
- Run U1's finder C2 (11:28 UTC) had already fetched the static-assets headers page.
- So the 10 fetches in U2 cover just 3 pages, and the same page was fetched again across runs as well.

**B-8ec0b5b5: a verified source lost between tasks**

- Task 34's note (preview-cpu.md) recorded only the dataset name and the grouping. It did not record the query or the field names.
- preview-cpu-trim task 9 cited that note as its method. It ran in session cd978dc2 on 2026-11-20, 14:22-14:30 UTC, and researched the fields again: one Context7 resolve, two query-docs calls, two WebSearches and two WebFetches.
- It still ended on secondary sources for `cpuTimeUs` and `scriptVersion`. Its own deviation says the API "was not queried live", and the spec told it to "confirm the field names before relying on them".
- The knowledge crossed flows: site-worker-new-strapi to preview-cpu-trim.

**B-34d901f9: investigators skip Context7**

I re-ran the census over agent `meta.json` and `.jsonl` files in the four project folders for 2026-11-14..20. Misses were counted by a 404, "Not Found" or error heuristic, so they are approximate.

| Agent type | Agents | Context7 calls | WebFetch | WebSearch | Remote curl | Misses |
|---|---|---|---|---|---|---|
| plan-investigator | 66 | 0 | 115 | 27 | 93 | 9 WebFetch, 4 curl |
| plan-worker-deep | 188 | 18 | 9 | 3 | 17 | not counted |
| plan-worker-lite | 198 | 2 | 0 | 0 | 2 | not counted |
| review-finder | 120 | 0 | 16 | 0 | 2 | not counted |
| review-verifier | 337 | 0 | 6 | 0 | 4 | not counted |

- The curl count includes Strapi API probes and OSV/Scorecard checks as well as doc fetches.
- The investigator WebFetch misses are all constructed developers.cloudflare.com, docs.strapi.io or raw GitHub paths:
  - partnerhub session 724defa2, 2026-11-19 10:03 UTC: three misses.
  - strapi-server session ac680855, 2026-11-16 08:09 UTC: six misses.
- Context7 does cover these platforms:
  - Task 9 resolved `/cloudflare/cloudflare-docs`.
  - A worker in session 44c4e644 resolved `/llmstxt/developers_cloudflare_workers_llms-full_txt`.

## The gap in the current text

- **research-methods SKILL.md:34** puts Context7 third, after the lockfile and the pinned source. A Cloudflare, Docker Hub or Strapi Cloud question has no lockfile, so the reader goes straight to "the official docs" and builds the URL by hand.
  - The useful steps exist, in web-doc-research.md:19-30: Context7 first, and `llms.txt` "instead of a search guessing at it". But that reference is gated on "a library, API, version…", and investigators did not read it.
- **plan-investigator.md:22** names Context7 only as an availability check. By contrast, plan-worker-deep.md:153 says "Context7 first … the web second, never from memory", and workers are the agents that use it.
- **review-finder.md:19, review-verifier.md:18 and review-v2-method SKILL.md:294** defer to research-methods. None says when to look something up, or that a sibling cell may already have the page.
- **Orchestrator-written text has no source rule at all:**
  - elaborate.md:39-47 (read, then write specs)
  - write-and-prove.md:74 and drain.md:402-419 (handover rows: no source field)
  - plan-flow.md:249 (a hand fix is recorded as addressed, and any new value it introduces is not checked)
  - The engines hand outside facts to investigators only for new dependencies (open-and-challenge.md:162).
- **The run-note block cannot carry sources.** run_note.rs:298 rejects an unknown key and reads the whole note as `malformed`, so a worker cannot record a source by itself.

## Design

**1. One platform rung in research-methods (B-34d901f9).**
- Edit SKILL.md:34 so a hosted platform or service with no lockfile goes Context7 → `<docs-root>/llms.txt` or WebSearch → fetch. The rule: a URL not taken from a search result, an llms.txt map or a link on a fetched page is a guess, and a 404 on one means search, never guess again.
- Add one imperative line to plan-investigator.md, mirroring the worker's line 153.
- Options:
  - (a) SKILL.md text alone. It reaches every research-methods consumer: investigators, finders, verifiers and research-*.
  - (b) (a) plus the investigator line. Recommended, because the worker evidence shows that an imperative in the agent body is what gets followed.
  - (c) A PreToolUse hook that refuses a WebFetch with no prior search in the transcript. Rejected: brittle, and it breaks legitimate fetches of known URLs.
- Tradeoff: Context7 is not a cure. Task 9 still ended on secondary sources, so the rule should state the order to try sources in, not promise an answer.

**2. Sourced orchestrator claims (B-98d14bca).**
- (a) Prose: add one role rule to both engine SKILLs and to plan-flow's "Fixed by hand". Any outside fact the orchestrator writes into a spec `detail`, a handover `action` or a hand fix must cite either a fetched source or a note `[[source]]` that covers the same object. Outside facts here are a CLI's capability, a console label, an API field, or an image tag or version. Otherwise the orchestrator must fetch it under research-methods or launch an investigator lens.
  - Cheap, but it relies on compliance, and it is exactly the step that slipped.
- (b) Structure: add an optional `sources` field (`[{url|note, claim}]`) on handover rows and in the spec. `plan check` would warn on an undone `credentials` or `external` handover without one.
  - This is a tomlctl schema, check and test change: `needs-plan`.
- (c) Probe existence. A version or tag pin introduced by a hand fix or an elaboration gets a premise or probe, such as a registry manifest lookup or `docker manifest inspect`, so F78-class errors fail at Prove instead of at the gate.
  - This builds on the probes Prove already has.
- Recommended: (a) plus (c) now; (b) only if (a) slips again.
- Note: task 34 shows that "cite a note" is not enough. The rule must say the source covers the same target, not an analogous one.

**3. Per-run source cache for review (B-6d3fb3e3).**
- Each cell already gets `run directory:` (review-v2.js:252). The design is `<run-dir>/sources/<url-slug>.md`, one file per URL holding the URL, the fetch time and an excerpt of the fetched text.
- A cell reads the file before fetching and writes it after. One file per URL avoids an append race between parallel cells. Run dirs are git-ignored in dev-tools (.gitignore:121), so page text is not committed.
- The cache carries raw page bytes, never a finding's reading of them. That keeps the verifier's independence, which the projection protects at fields.rs:1725.
- Options:
  - (a) The cache, run-scoped only.
  - (b) An orchestrator pre-fetch at survey. Rejected: it cannot predict pages.
  - (c) Accept the repeats.
- Tradeoff: the measured cost is small (10 fetches), so this is the lowest-value item of the four. A cached page is also untrusted data a second time.

**4. Sources that outlive a task (B-8ec0b5b5).**
- Step 1: add `[[sources]]` (`url` or `query`, `claim`, `how_verified`) to the run-note block in run_note.rs and schema.rs, with tests. Only after that, tell workers (plan-worker-deep.md:153, plan-worker-lite.md:156) to record any exact outside value they settle there.
  - Ordered steps, and adding the instruction first would break notes: `needs-plan`.
- Step 2, the drain promotes the sources. Options for where to:
  - (a) A flow-level `docs/flows/<slug>/notes/sources.md`. Not enough here, because the repeat crossed flows.
  - (b) A repo-level reference such as `docs/references/<topic>.md`, or a tomlctl-backed store. Investigators, elaboration and workers would read it before searching. It would be promoted at drain the way tangentials become backlog rows, with a `backlog check`-style dedupe.
- Recommended: (b), with the drain as the only writer, matching the backlog's single-writer rule.

## Findings

- **Library/API or file**: claude/skills/research-methods/SKILL.md:34 (Source hierarchy); claude/agents/plan-investigator.md:22
- **Source**: SKILL.md:34; web-doc-research.md:19-30; transcript census 2026-11-14..20 (plan-investigator: 0 Context7 against 115 WebFetch, 9 WebFetch misses on constructed URLs)
- **Evidence-grade**: high
- **Finding**: The hierarchy has no platform rung, and the investigator is never told to use Context7 or a search first. Add both.
- **Details**: Workers, whose body says "Context7 first" (plan-worker-deep.md:153), made 20 Context7 calls. Investigators, whose body does not, made none. Context7 does index Cloudflare docs (`/cloudflare/cloudflare-docs`).
- **Counter**: If investigator sessions had no Context7 tools loaded, the cause would be the grant or loading, not the text. Workers in the same window and project did load them, which argues against that, but I did not check each investigator's ToolSearch calls.
- **Impact on plan / report**: A text-only task touching SKILL.md and plan-investigator.md.

- **Library/API or file**: claude/skills/plan-v2-engine/references/elaborate.md:39; write-and-prove.md:74; claude/skills/implement-v2-engine/references/drain.md:402; claude/skills/review-v2-engine/references/plan-flow.md:249
- **Source**: partnerhub_pages commit f1ab790 (task 34 detail); sessions 36cbd3f6 and b2a1b292 (user reports on H2 and H5); strapi-server plan.toml gate round A0 (`oven/bun:1.4.5: not found`)
- **Evidence-grade**: high
- **Finding**: Outside facts written by the orchestrator carry no source requirement. Add a role rule with a same-target clause, plus an existence probe for version and tag pins (`needs-plan` if the `sources` field is added).
- **Details**: There were three wrong-fact incidents. Each was caught by a worker, the user or the gate, and none at write time.
- **Counter**: If the engines already required sourcing in a file I did not read, such as an engine SKILL role rule, this would be a compliance gap, not a text gap. A grep of both engine SKILLs and all their references for Context7, fetch and source terms found none.
- **Impact on plan / report**: Prose task(s) in both engines and plan-flow. Optionally a tomlctl handover or spec field with a check.

- **Library/API or file**: tomlctl/src/plan/run_note.rs:298 (unknown-key rejection)
- **Source**: run_note.rs:1-9 and :298; partnerhub preview-cpu-trim task 9 note and session cd978dc2
- **Evidence-grade**: high
- **Finding**: Run notes cannot record sources: an added key turns the note `malformed`. Parse a `[[sources]]` table first, then instruct workers and promote at drain (`needs-plan`).
- **Details**: The files are run_note.rs, schema.rs (`RunNote` and the attempt fields), their tests, plan-worker-deep.md:153, plan-worker-lite.md:156 and drain.md (promotion). The re-research crossed flows, so the store must be repo-level.
- **Counter**: If workers recorded sources in prose and a reader used them, no parser change would be needed. Task 34's prose named the dataset but not the fields, and task 9 still re-researched.
- **Impact on plan / report**: A tomlctl task before the agent-text task.

- **Library/API or file**: claude/workflows/review-v2.js:252 (run directory in the preamble); tomlctl/src/review/fields.rs:1725
- **Source**: session 724defa2, cells C13 and C17 and verifiers of F65 and F82: 10 fetches of 3 pages
- **Evidence-grade**: high
- **Finding**: Add a per-run source cache under `<run-dir>/sources/`, one file per URL, holding raw excerpts only.
- **Details**: No Workflow change is needed. The text changes go in review-finder.md, review-verifier.md and the review-v2-method SKILL near :294.
- **Counter**: If verifier independence required its own fetch, sharing would weaken refute-first. Shared page bytes are evidence, not conclusions, and the projection's withholding (fields.rs:1725) covers only grade and fix.
- **Impact on plan / report**: Low value; a candidate to drop if the plan is over budget.

Coverage:
- **Read in full**: the 4 backlog rows; the brief; plan-investigator.md; research-methods SKILL.md and web-doc-research.md.
- **Read in part**:
  - Agents: the source lines of plan-worker-deep/lite.md and review-finder/verifier.md.
  - Engine text: elaborate.md:36-80; the write-and-prove.md handovers section; the drain.md section index and handovers section; plan-flow.md:249.
  - tomlctl: run_note.rs:1-12 and :290-300.
- **Consumer evidence**:
  - partnerhub_pages: the task 34 and task 9 specs and notes; the H2 and H5 text at each commit; F65 and F82.
  - strapi-server: F78 and gate round A0.
- **Transcripts**: transcript tool and URL extraction over 9 named agent transcripts, plus a census across all four project folders.
- **Ruled out**:
  - Investigators lacking the Context7 grant: the tools frontmatter includes it.
  - The finder citing a URL the verifier can see: not checked. The `evidence` field may carry it, but `evidence` holds file:line facts and output, so the verifier would not reuse it as a cache.
- **Paraphrase only**: no web lookups were needed. Every claim rests on repo text or transcripts, which are paraphrased here.

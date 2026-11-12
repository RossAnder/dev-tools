Verdict: The three agent files define the notes (claude/agents/plan-investigator.md:28, plan-challenger.md:35, plan-cold-reader.md:34). Only prose reads them: the plan-v2-engine references read the first line (open-and-challenge.md:42) and a person or model reads the body. No code parses past line 1. A sources block fits anywhere after the Verdict line, uses repo-relative paths, records file state as well as HEAD, and needs no Workflow, tomlctl or test change.

## Answer

**Who defines the notes.** Each agent file has a "Write the note" paragraph that sets the first line and the body. These paragraphs are agent-specific, not shared blocks:

- `claude/agents/plan-investigator.md:28`: the body is the finding records, then a coverage line.
- `claude/agents/plan-challenger.md:35`: `no restructure` or `restructure`.
- `claude/agents/plan-cold-reader.md:34`: the unactionable points, the false premises and the check output.

All three also carry the shared `plan-v2-note-writer` block, at `plan-investigator.md:92`, `plan-challenger.md:55` and `plan-cold-reader.md:54`. That block says the note is written in one Write call and is public text.

**Who sets the paths.**

- `claude/workflows/plan-v2-investigate.js:71` gives `notes/<lens>.md`.
- `claude/workflows/plan-v2-prove.js:77` gives `notes/cold-read-<m>.md`.
- `open-and-challenge.md:121,129` gives `notes/structure-challenge.md`.
- `tomlctl/src/plan/open.rs:85-90` writes `notes/backlog-<id>.md` itself. That one is a different kind of note, with no `Verdict:` line.

**Who reads the notes.**

- **The Workflows never open a note.** They check only the agent's return object against a schema with `additionalProperties: false` (investigate.js:22-30, prove.js:21-29), plus a suffix match on the path (investigate.js:80-84).
- **Only the `Verdict:` first line is read mechanically, and it is read in prose.** These are instructions to the planning model, not code:
  - `claude/skills/plan-v2-engine/references/open-and-challenge.md:42`, `:77`, `:134`, `:226` and `:234`
  - `claude/skills/plan-v2-engine/references/write-and-prove.md:278` and `:346`
  - the PowerShell probe regex `^Verdict: ` at `docs/ideas/plan-v2-probes.md:192`
- **The body is read by the planner.** `open-and-challenge.md:85` and `plan-v2-engine/SKILL.md:18` say "a note is where to look, not the evidence", and `write-and-prove.md:304` says the same for cold-read results.
- **Three agents are told never to read notes:**
  - `claude/skills/review-v2-method/references/plan-records.md:19`
  - `plan-challenger.md:23`
  - `plan-cold-reader.md:20`
- **Two whole-directory passes touch the bytes but do not parse them.**
  - The secret scan at `approve.md:55` greps every note for `C:[\\/]Users[\\/]`.
  - The fan-out check digests untracked notes (`fanout-check.sh:40`).
- **Nothing in `tomlctl/src` (other than `open.rs`'s backlog notes), `glimpse/src` or `tomlctl/tests` reads a note.** `rg "notes/"` finds no reader there, and none of `plan-investigator`, `plan-challenger` or `plan-cold-reader` appears in a test.

**What a sources block must respect so the existing readers keep working.**

1. The block must not come first. Line 1 stays `Verdict: <one line>`, because the readers above key on that line and treat a note that does not open with it as never written. Put the block after the Verdict line and the answer, or at the end.
2. Paths must be repo-relative and forward-slashed. An absolute Windows path trips the approval secret scan, and the note is committed publicly.
3. "Commit read at" alone does not reproduce what the agent read. Agents read the working tree, and the working tree is often dirty. At launch, the brief and every note are untracked (`open-and-challenge.md:91`), and this session's tree has `docs/ideas/v2-pipeline.md` modified. Each entry needs a file-state field next to the HEAD sha, either the blob id from `git hash-object <path>` or a `dirty` flag. Otherwise `git show <sha>:<path>` shows text the agent never saw.
4. Define the block in one shared block, not in `research-finding-record`. That block is shared with `research-deep` and `research-lite` (`scripts/shared-blocks.toml:121-126`), so changing it would change v1 research output. The natural home is `plan-v2-note-writer`, which all three plan agents carry (`shared-blocks.toml:144-153`). Its three copies must stay byte-identical (the pre-commit verifier checks this).
5. The return objects and the Workflow schemas need not change. Adding a key to a return object would require changing both schemas, because of `additionalProperties: false`.
6. Fenced examples in an agent file are linted when they contain a `tomlctl` command (`tomlctl/src/cli/dispatch/tests/lint.rs:42-55` collects `claude/agents/*.md`). A pure TOML/fenced sources example with no `tomlctl` line is unaffected. An agent `.md` edit triggers the `cli::dispatch::tests` hook step.

## Findings

- **Library/API or file**: `claude/skills/plan-v2-engine/references/open-and-challenge.md:42` (pending-lens rule)
- **Source**: file read; `rg -n "Verdict:"` over `claude/`, `tomlctl/src` and `glimpse/src`
- **Evidence-grade**: high
- **Finding**: The only mechanical reader of a note is a first-line `Verdict:` test, and every instance of it is prose (the engine references) or the probe doc's PowerShell regex. A sources block after line 1 breaks none of them.
- **Details**: The sites are `open-and-challenge.md` lines 42, 77, 134, 226 and 234, `write-and-prove.md` lines 278 and 346, and `plan-v2-probes.md:192`, which is `-notmatch '^Verdict: '`. The `rg` sweep finds no Rust, JS or test code that opens a note body. The challenger's keep/remove verdict form (`plan-v2-probes.md:225,410`) concerns line 1 only.
- **Counter**: A reader outside `claude/`, `tomlctl/src`, `glimpse/src`, `scripts/` and `docs/ideas` that parses note bodies, such as an installed copy under the Claude config dir that differs from the repo.
- **Instances**: open-and-challenge.md:42,77,134,226,234; write-and-prove.md:278,346; plan-v2-probes.md:192,289,298. Sweep was `rg -n "Verdict:"` and `rg -n "notes/"`. Complete for the tree.
- **Impact on plan / report**: The block's position is free after line 1, and no reader change is needed to keep the existing readers working.

- **Library/API or file**: `claude/workflows/plan-v2-investigate.js:22-30` (`RETURN_SCHEMA`), `claude/workflows/plan-v2-prove.js:21-29`
- **Source**: file read
- **Evidence-grade**: high
- **Finding**: The Workflows validate only the return object, with `additionalProperties: false`, and never read the note. A sources block written into the note needs no Workflow change. Adding sources to the return object would need both schemas, and the challenger's `{verdict, note}` in the engine, to change.
- **Details**: `toEntry` (investigate.js:86-104, prove.js:95-115) uses only `ret.id`, `ret.verdict`, `ret.note` and the counts. `namesNote` matches by suffix after normalising `\` to `/`.
- **Counter**: The Workflow runtime reading note files on its own account, which nothing in either script does.
- **Impact on plan / report**: Keep the sources in the note, not the return. Glimpse reads them from disk.

- **Library/API or file**: `claude/skills/plan-v2-engine/references/approve.md:55` (secret scan)
- **Source**: file read
- **Evidence-grade**: high
- **Finding**: Absolute Windows paths in a sources block would be reported as gate hits on every approval. The block must hold repo-relative, forward-slashed paths.
- **Details**: The pattern includes `C:[\\/]Users[\\/]` and runs over all of `docs/flows/<slug>/`, notes included. The `plan-v2-note-writer` block already says "no private paths". The investigate Workflow likewise normalises `\` before matching paths.
- **Counter**: The scan pattern changes to exclude notes, or the block stores paths in a form the pattern cannot match.
- **Impact on plan / report**: The block contract says "repo-relative path, `/` separators".

- **Library/API or file**: `claude/skills/plan-v2-engine/references/open-and-challenge.md:91` (untracked brief and notes at launch); `git status --short` here shows ` M docs/ideas/v2-pipeline.md` at HEAD `207a03a1`
- **Source**: file read, and the command's output in this session
- **Evidence-grade**: high
- **Finding**: A HEAD sha alone does not pin "the source as it was when the note was written". Agents read the working tree, which at plan time is routinely dirty or untracked. Record a per-file state as well: the blob from `git hash-object <path>`, or a `dirty` or `untracked` mark.
- **Details**: The brief and the notes are untracked until approval (`approve.md:115`). An investigator citing a dirty file at HEAD X would send glimpse to `git show X:<path>`, which is the wrong text. A blob id is retrievable only if the file was later committed unchanged, so glimpse also needs a "source changed since read" fallback.
- **Counter**: Every cited path being clean at read time. This session shows that does not hold.
- **Impact on plan / report**: The schema has fields `path`, `line`, `claim`, `commit` and `blob` (or `state`). The preview compares the recorded blob against the present one.

- **Library/API or file**: `scripts/shared-blocks.toml:121-126` (`research-finding-record`) and `:144-153` (`plan-v2-note-writer`)
- **Source**: file read
- **Evidence-grade**: high
- **Finding**: Define the sources-block contract in `plan-v2-note-writer`, which all three plan agents share, or in each agent's own "Write the note" paragraph. Do not put it in `research-finding-record`, which `research-deep` and `research-lite` also carry, where it would change v1 research output.
- **Details**: Each block's carriers must stay byte-identical (`scripts/verify-shared-blocks.sh` runs at pre-commit). The investigator's finding record already has a free-text `Source` line. The challenger and the cold reader have no structured record at all, so the block is new to them.
- **Instances**: `claude/agents/plan-investigator.md:92`, `plan-challenger.md:55` and `plan-cold-reader.md:54` (the note-writer block); `plan-investigator.md:28`, `plan-challenger.md:35` and `plan-cold-reader.md:34` (the per-agent note paragraphs). Complete.
- **Counter**: A requirement that v1 research reports carry the same block, which would justify editing `research-finding-record`.
- **Impact on plan / report**: The edit touches the three agent files plus whatever parser glimpse or tomlctl adds. No engine reference needs to change for the existing readers to keep working.

Coverage: I read in full:

- the "Write the note" sections and shared blocks of `plan-investigator.md`, `plan-challenger.md` and `plan-cold-reader.md`
- the note-handling code of `plan-v2-investigate.js` and `plan-v2-prove.js`
- the note references in `open-and-challenge.md`, `write-and-prove.md`, `approve.md` and `SKILL.md`
- `shared-blocks.toml:60-160`
- `plan/open.rs:75-100,175-195` and `plan/store.rs:40-60`

Checks run:

- `rg "Verdict:"` and `rg "notes/"` over `claude/`, `tomlctl/src`, `glimpse/src` and `scripts/`
- `rg` for the agent names in `tomlctl/tests`
- `git status` and `git rev-parse`

Candidates ruled out:

- The decision `evidence` field (`plan/schema.rs:182`) is a free string and parses no note.
- `plan check`'s `design_section` (`check.rs`) reads only `design/`.
- `backlog-<id>.md` notes are a different kind of note, with no Verdict line.
- glimpse reads no notes today, as the brief also states.

Outside this lens: the worker run notes (`notes/run/`) belong to the `worker-note` lens.

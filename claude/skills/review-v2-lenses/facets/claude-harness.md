+++
id = "claude-harness"
status = "stable"
families = ["code"]
apply_when = { signals = ["harness:claude"] }
attaches_to = ["hygiene", "doc-drift", "ripple"]
+++

# Claude Code harness

## General

Harness files are prose that a model executes, so a defect rarely fails loudly: a broken frontmatter, a missing tool grant or a dangling skill name degrades into an agent that improvises. Judge each file by what the harness loader and the dispatching model will actually see. Gates a repository already runs, such as a shared-block parity script or a carrier lint, own the mechanical half; do not re-derive what their output already reports.

- **Frontmatter is the contract.** A command needs `description`, and `argument-hint` when its body reads `$ARGUMENTS`. A skill needs `name` equal to its directory name and a `description` that says when to use it, because the description is the only text the model sees before invoking it. An agent needs `name`, a `description` that tells the dispatcher when to pick it, and a `tools` list. YAML that fails to parse, most often an unquoted `: ` inside a long description, drops the whole block without an error.
- **`tools` is the whole grant, and it fails silently.** A body that tells an agent to call a tool absent from its `tools` line (WebFetch, a `Skill` load, an MCP tool) gets no load-time error, only an agent that cannot do the step. MCP names must be the exact `mcp__<server>__<tool>` spelling, and a granted MCP tool can stay hidden until ToolSearch surfaces it, so a body that says "if the tool is absent, skip" without a ToolSearch step quietly skips.
- **Cross-references resolve by name, never by import.** "Invoke the `x` skill", `Skill(x)`, a `subagent_type`, a `/command` mention, a `references/<file>.md` link and a `#heading` anchor are all plain text, and nothing checks them until a model follows one and finds nothing.
- **Stubs read as complete.** A phase or step heading whose body is one sentence pointing elsewhere, "TBD", a numbered sequence that skips a step, or "see below" with nothing below it hands the model a gap it will fill on its own.
- **Terminology is shared state.** Carriers that dispatch the same agents, write the same store or quote the same block must name each concept, file and status the same way; one carrier calling a status `done` where its peers say `completed` is a contract split, not a style choice.
- **Deployment is a copy or a link.** Entries are linked or copied into the user's Claude directory and sometimes mirrored (for example into `.github/`), so adding, renaming or removing one leaves a deploy step, a mirror link or a stale installed copy behind.

## By lens

### For hygiene

- Do not count an agent, command or skill as dead because no file names it: the harness discovers all three by directory, and the user invokes commands by typing them. Dead harness code is what nothing can reach: a `references/` or `scripts/` file inside a skill that its `SKILL.md` and siblings never link or run, or a helper script no hook, command or settings entry invokes.
- An agent whose description says it is dispatched only by named carriers is reachable only through those carriers; when a whole-tree search finds none of them, escalate it as a dead-code candidate rather than writing a removal, since a user or another repo's carrier may still dispatch it.
- Unused configuration: a tool in an agent's `tools` line that its body and every skill it loads never use, an `allowed-tools` entry for a command that no longer runs that tool, a settings hook or permission naming a script or tool that no longer exists. A broad grant may be deliberate (a teammate-delivery tool, a shared tool list kept identical across a lite/deep pair), so read the pair before writing the row.
- A leftover after a removal: the manifest entry, deploy-script line, mirror link or parity-manifest row for a removed agent, skill or block.

### For doc-drift

- A description that promises verbs, flags, phases or outputs the body or the tool no longer has. A skill's description is the trigger text, so drift there misroutes invocations, not just readers.
- An `argument-hint` that disagrees with how the body parses `$ARGUMENTS`: flags the body does not accept, or positional forms it rejects.
- Prose that restates a value another file owns, such as a model name, an effort level, a tool list, a flag table or a status vocabulary, and no longer matches that owner. Quote the restatement and cite the owning line.
- A command, flag or verb in a fenced example that the current tool rejects or that a project guidance file forbids; check it against the tool's own help or reference, not against memory.
- A procedure whose step order, phase names or stop conditions diverge from the skill or contract the body says it follows.
- Section headers missing or out of the order the file's type or a sibling family establishes (a command family whose peers all carry the same phase list), where the missing section leaves a reader without a step the peers define.

### For ripple

- A renamed or removed skill, agent or command id: search the old id as `Skill(` arguments, "the `<id>` skill" prose, `subagent_type` values, `/<id>` slash mentions, settings files, deploy scripts, mirror directories, parity manifests, CLAUDE.md files and other skills' descriptions.
- A renamed heading: other files' `#anchor` links and "see <Section>" or "Step <n>" citations; renumbering steps breaks every cross-file "Step 4.5" reference, and a heading some tool reads as a key changes that key.
- A rule changed in one carrier that peer carriers state too: a lite/deep agent pair, a command family sharing an apply or vet procedure, or carriers quoting the same shared block. List the unchanged peers in `instances`.
- A new agent, skill or command with no carrier that dispatches, loads or documents it, where it is not reachable by user invocation or model discovery, is an unwired seam; escalate it.
- A tool, flag or verb a change renamed in the CLI the harness drives, still spelled the old way in carrier prose or fenced examples.

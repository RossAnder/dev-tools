# Dispatch discipline reference

Detail behind Step 4 of the apply pipeline: how the orchestrator launches the round's cluster agents.

### Dispatch discipline

**File-cluster grouping is the primary conflict-avoidance strategy.** No two agents may edit the
same file. Findings that cannot be split into non-overlapping clusters get **sequenced, not
parallelised**; `isolation: "worktree"` is a last resort only — worktree merges are slow and risk
losing work.

**You MUST make all independent file-cluster Agent calls in a single response message.** Emit one
message containing every Agent tool-use block so they execute concurrently. **Do NOT reduce the
agent count** — launch the full complement. Dependent same-file agents run sequentially after the
parallel batch. Nothing is committed between rounds (the apply-constraints no-auto-commit rule), so
the Step 5.5 rollback reverts the whole run's work, never one round's.

**Dev server (UI clusters).** When a cluster's items change something visible — a component, a
style, a template — and the project's `.mcp.json` declares the `playwright` server, probe the dev
URL the project documents (its CLAUDE.md, or the dev script in its manifest). If nothing answers
and a dev command is documented, start it once in the background before the round launches;
stop a server this run started before Step 5. Put `DEV SERVER: <url>` in every cluster prompt, or `DEV SERVER: none`;
the implementers attach only to that URL and never start a server of their own.

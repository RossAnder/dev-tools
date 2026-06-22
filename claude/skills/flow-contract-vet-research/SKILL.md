---
name: flow-contract-vet-research
description: Universal vet-pass procedure for research-agent output — the orchestrator-side gate that distinguishes "research returned" from "research findings are trustworthy". Triages by evidence-grade, honours ESCALATE-TO-DEEP flags, drops unverified low-confidence findings, spot-checks sampled findings against cited file:line / URLs / library versions, downgrades or drops fabrications, appends a durable [[vet_events]] ledger entry, emits a mandatory per-agent console summary line, and escalates `research-lite`→`research-deep` on >30% systemic failure. Consult immediately after a research-lite or research-deep agent returns, before persisting findings to any ledger or notes section.
---

**Vet research-agent output (orchestrator).** This block defines the universal vet-pass procedure the orchestrator runs after research-agent dispatch returns. The build/test verification agent catches code-shape failures, but it does NOT catch fabricated `file:line` references, made-up library version pins, or low-confidence claims dressed up as fact in research output. The vet pass is the gate that distinguishes "research returned" from "research findings are trustworthy."

1. **Triage by source agent + evidence-grade.** Group findings by `(agent_index, evidence-grade)`; emit a one-line summary per group to console.
2. **Honour `ESCALATE-TO-DEEP` flags.** If any agent prefixed its return with `ESCALATE-TO-DEEP: <reason>`, re-dispatch that lens to `research-deep` with the escalation reason in the prompt before further vetting that lens's output.
3. **Drop unverified `low` / `low-confidence` findings** unless explicitly framed as a hypothesis with a concrete verification step.
4. **Spot-check sampled findings.** Sample size per carrier — see carrier prose around this block. For each sampled finding: read the cited `file:line`, confirm the code matches the description, verify any cited URLs / library version pins / Context7 IDs.
5. **Drop or downgrade findings that fail vetting**, with rationale. Downgrade by appending `_orchestrator-downgrade: <reason>` to the evidence-grade line.
6. **Append a durable `[[vet_events]]` entry to the ledger** via the canonical heredoc form — one entry per vetted agent, the `agent_index` field discriminates:

   ```bash
   cat <<'EOF' | tomlctl array-append <ledger> vet_events --json -
   {"timestamp":"<ISO 8601>","command":"<review|optimise|review-plan|plan-new|plan-update|test-bootstrap>","agent_index":<n>,"lens":"<lens>","sampled_count":<N>,"dropped_count":<M>,"downgraded_count":<K>,"dropped_ids":["<R{n}>",...],"rationale":"<≤8 KiB rationale>"}
   EOF
   tomlctl set <ledger> last_updated <YYYY-MM-DD>
   ```

   `array-append` is a `mutate_doc*`-routed verb, so this idiom works against a fresh (missing) ledger too — the first `vet_events` append auto-creates the file with the `schema_version = 1` skeleton, no pre-initialisation needed.

   See the `flow-contract-ledger-schema` skill → Vet event log section for the full field set.
7. **Emit the mandatory console line per agent**: `vet: Agent-{n} (<lens>) — N findings sampled, M dropped, K downgraded`. The format is fixed; lens names are carrier-specific (see carrier prose).
8. **>30% systemic failure rule.** If more than 30% of an agent's findings fail vetting, re-dispatch that lens with the failure pattern in the prompt. For `research-lite` agents, the re-dispatch SHOULD escalate to `research-deep` (the systemic failure indicates the lens is too judgement-heavy or fabrication-prone for a fetch-and-summarise pass on this profile).

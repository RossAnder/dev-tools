<!-- Generated from execution-record.toml. Do not edit by hand. -->

# glimpse — live task-DAG TUI for tomlctl flows, fed by hook-recorded agent records — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | ignore-agent-records-and-the-glimpse-build-dir-in-git | 2026-09-29 | | 1 file |
| E3 | accept-the-colon-inside-bold-field-label-spelling | 2026-09-29 | | 1 file |
| E4 | scaffold-the-glimpse-crate | 2026-09-29 | | 32 files |
| E5 | gate-glimpse-formatting-in-pre-commit | 2026-09-29 | | 1 file |
| E7 | wrap-the-herdr-cli | 2026-09-29 | | 1 file |
| E10 | join-the-execution-record-for-the-snapshot | 2026-09-29 | | 2 files |
| E12 | resolve-junction-glyphs | 2026-09-29 | | 1 file |
| E13 | add-the-agents-store-schema | 2026-09-29 | | 3 files |
| E15 | add-config-and-theme | 2026-09-29 | | 2 files |
| E17 | order-diagram-layers-to-reduce-crossings | 2026-09-29 | | 1 file |
| E19 | discover-flows-and-pick-the-freshest | 2026-09-29 | | 1 file |
| E21 | ensure-one-glimpse-pane-per-tab | 2026-09-29 | | 1 file |
| E24 | assemble-the-task-snapshot | 2026-09-29 | | 2 files |
| E26 | correlate-subagents-with-flow-tasks-from-claude-codes-on-disk-state | 2026-09-29 | | 2 files |
| E28 | assign-diagram-coordinates | 2026-09-29 | | 1 file |
| E30 | install-hooks-and-the-herdr-keybinding | 2026-09-29 | | 1 file |
| E32 | handle-hook-events | 2026-09-29 | | 1 file |
| E34 | define-the-snapshot-model | 2026-09-29 | | 2 files |
| E36 | record-hook-events-into-the-agents-store | 2026-09-29 | | 3 files |
| E38 | compute-snapshot-change-sets | 2026-09-29 | | 1 file |
| E39 | route-diagram-edges-through-inter-layer-channels | 2026-09-29 | | 1 file |
| E41 | add-the-agents-list-verb-and-group-dispatch | 2026-09-29 | | 3 files |
| E43 | poll-the-flow-files-and-fetch-snapshots | 2026-09-29 | | 1 file |
| E45 | wire-the-new-verbs-into-the-cli | 2026-09-29 | | 3 files |
| E47 | hold-app-state-and-map-keys | 2026-09-29 | | 2 files |
| E49 | render-the-header-strip | 2026-09-29 | | 1 file |
| E50 | release-tomlctl-0120-with-the-new-features-registered | 2026-09-29 | | 4 files |
| E51 | document-the-agents-verb-group | 2026-09-29 | | 8 files |
| E53 | integration-test-tasks-snapshot | 2026-09-29 | | 1 file |
| E54 | document-the-snapshot-verb-and-the-agents-store-writer | 2026-09-29 | | 4 files |
| E55 | integration-test-agents-record | 2026-09-29 | | 1 file |
| E56 | pick-a-flow-from-the-selector | 2026-09-29 | | 1 file |
| E57 | render-the-layer-view | 2026-09-29 | | 1 file |
| E59 | render-task-details-with-markdown | 2026-09-29 | | 2 files |
| E60 | tail-the-selected-agents-transcript | 2026-09-29 | | 2 files |
| E62 | render-the-traversal-view | 2026-09-29 | | 1 file |
| E69 | paint-and-navigate-the-diagram | 2026-09-29 | | 2 files |
| E78 | compose-the-frame-and-switch-views | 2026-09-29 | | 1 file |
| E79 | run-the-event-loop | 2026-09-29 | | 1 file |
| E81 | parse-arguments-and-wire-the-subcommands | 2026-09-29 | | 3 files |
| E83 | document-glimpse | 2026-09-29 | | 3 files |
| E86 | correct-the-agents-timestamp-doc-comment | 2026-09-29 | | 1 file |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E6 | Block comment in .githooks/pre-commit states the tomlctl fmt step never covers glimpse; block itself is as specified | 2026-09-29 | | It fires on a staged glimpse .rs but checks only the tomlctl crate via --manifest-path, so glimpse formatting was never checked; the new gated block is the only glimpse fmt check | — |
| E8 | Herdr::focus takes (pane_id, FocusDir) in glimpse/src/herdr.rs | 2026-09-29 | | herdr pane focus requires --direction and moves focus to a neighbour; herdr 0.9.1 has no focus-by-id | — |
| E9 | Herdr::layout looks the pane up by id in result.layout.panes[] | 2026-09-29 | | herdr pane layout --pane <id> returns the whole tab layout, so the rect must be selected by pane_id | — |
| E11 | checkpoint_facts assigns a train commit that matches no row to every checkpoint in that entry's checkpoint_ids | 2026-09-29 | | The spec gave no rule for commits matching no row (fixups, docs); attributing them to the entry's checkpoints loses none | — |
| E14 | agents.toml segments are written as [[agents.segments]] array-of-tables; the reader accepts both forms | 2026-09-29 | | io::write_toml_with_sidecar uses toml::to_string_pretty, which always writes a non-empty array of tables as [[...]] blocks; toml::Value cannot mark a table inline without changing the shared writer or adding toml_edit | — |
| E16 | Config treats unknown keys, wrong types and out-of-range values as malformed (defaults + warning); Theme adds a warning style and ACCENT_BG | 2026-09-29 | | Strict validation reports a typo instead of silently ignoring it; the header needs a style for the config warning | — |
| E18 | Ordered.chains is Vec<Chain> carrying edge id, the Edge and its top layer; dummies tie-break after tasks by (edge, step) | 2026-09-29 | | route receives only &Ordered and needs each edge's kind and direction for coupling buses and arrowheads; dummies also need a deterministic tie order | — |
| E20 | rank sorts in place and freshest returns Option<&FlowEntry>; parse_flows takes an mtime lookup closure | 2026-09-29 | | tomlctl flow list output carries no tasks_mtime, so the mtime comes from a filesystem lookup injected for testability; slug breaks mtime ties deterministically | — |
| E22 | Focus of an existing glimpse pane is derived from adjacent rects via herdr pane focus --direction; best-effort, focused=false when not adjacent | 2026-09-29 | | herdr 0.9.1 has no focus-by-pane-id; focus moves only to a neighbour in a direction | — |
| E23 | launch_line leaves the path bare only for [A-Za-z0-9:/._-]; otherwise & '<exe>' with ' doubled | 2026-09-29 | | pwsh expands $ and chokes on ( & ' in a bare token, so whitespace alone is not a safe test | — |
| E25 | snapshot resolves agents.toml, execution-record.toml and context.toml as siblings of store_path; slug comes from the argument | 2026-09-29 | | Sibling resolution keeps a --file store and its companions together with no second slug resolution; Store has no slug field | — |
| E27 | Dispatch regex matches task ids with [0-9]+ rather than the digit class | 2026-09-29 | | tomlctl builds regex with default-features = false and no Unicode classes, so the digit class does not compile; the ASCII digits matched are identical | — |
| E29 | Positions carries a width map and a centre(slot) method | 2026-09-29 | | route and paint need each slot's width to find edge attachment points and lay out labels | — |
| E31 | merge_settings returns Result<(Value, added events)>; herdr config path follows herdr's own resolution order; apply_herdr refuses a merge that breaks a parseable config | 2026-09-29 | | A malformed settings file must be rejected rather than overwritten, and the added-event list feeds the dry-run report; HERDR_CONFIG_PATH and XDG_CONFIG_HOME are what herdr itself loads | — |
| E33 | decide takes (result, herdr_pane_id, cwd) as plain arguments | 2026-09-29 | | cwd comes from the payload, not the environment; plain arguments keep decide pure without set_var, which is unsafe in edition 2024 | — |
| E35 | Index lookups task, agents_for, record_for and checkpoint take (snapshot, id) | 2026-09-29 | | Index must stay owned with no borrow of the Snapshot, so returning row references needs the snapshot passed in | — |
| E37 | record writes through mutate_doc_conditional; Stop carries agent_type/kind/name/team; Start flow order checks the flow already holding the agent before session affinity | 2026-09-29 | | A no-op repeat start must not rewrite the file; a Stop that outruns its Start writes a complete row; a resume with no new dispatch stays in the agent's own flow | — |
| E40 | A channel gains one leading row when an upward edge enters the upper layer; Routed adds arrows and layer_row() | 2026-09-29 | | An upward edge's arrowhead must sit next to the upper layer, which would otherwise land on track 0; the painter needs arrowhead cells and each layer's row | — |
| E42 | dispatch_record takes harness as &str parsed by Harness::parse; list honours --strict-read with a not_found error | 2026-09-29 | | The clap types do not exist yet so a plain &str is the neutral argument; strict-read parity with the backlog store | — |
| E44 | Source sends FlowsChanged on the first scan and runs the capabilities probe on the poller thread | 2026-09-29 | | The runtime lists flows only on FlowsChanged, so without a baseline event the selector stays empty; probing on the thread keeps Source::start non-blocking | — |
| E46 | AgentsOp is not re-exported from cli/mod.rs; tasks snapshot --file takes the slug from the store's parent directory; orchestrator lifted the pre-wiring dead-code allows and relabelled the payload error to PAYLOAD | 2026-09-29 | | The only consumer imports from cli::types, so a re-export would be unused; the flow directory name equals the slug; once wired, the allows in agents/mod.rs, tasks/mod.rs, snapshot.rs and snapshot_record.rs were stale and dispatch.rs named a --payload flag that does not exist | — |
| E48 | App adds resolved_orientation and stale_after fields and a tick(now) method; apply returns Option<Action> for Quit and SwitchFlow | 2026-09-29 | | o needs a base orientation before any override; staleness and flash expiry happen without a new snapshot; the runtime needs Quit/SwitchFlow back from apply | — |
| E52 | write.md seeded-file list now matches SCHEMA_SEEDED_FLOW_FILES (adds backlog.toml, tasks.toml, agents.toml, drops the count); flow.md qualifies the tasks snapshot integrity entry | 2026-09-29 | | The existing list was already stale against io.rs; snapshot applies read options to every companion file it reads | — |
| E58 | Vertical layer navigation moves Up/Down in reading order across layers and Left/Right between layers; agent chip elapsed time parsed by a local RFC 3339 parser | 2026-09-29 | | In a vertical list a layer's rows run along the same axis as the layers, so the literal mapping makes Down skip rows; the crate has no date library | — |
| E61 | TailState adds tokens and rejected fields; activity.rs adds parse_utc | 2026-09-29 | | agents.toml fills context_tokens only on stop, so a running agent needs the tail's latest usage; rejected lets the panel say why it is empty; glimpse had no RFC 3339 parser | — |
| E63 | Traversal along-axis moves walk the centre task's layer rather than the side column | 2026-09-29 | | The centre is always app.selected, so walking a side column after crossing needs a trail field on App, which is outside the task's files | — |
| E70 | Horizontal glyphs reflect the resolved character across the diagonal; out-edges use theme.coupling_edge | 2026-09-29 | | Reflecting keeps the layer-axis line unbroken at crossings in both orientations; the theme has no dependents style | — |
| E76 | Checkpoint B train attributes each glimpse file to the task that filled it and also commits the completed C-group modules | 2026-09-29 | | The scaffold's Files line claims every module, which would collapse the crate into one commit; main.rs declares every module, so B's tip builds from git only if the completed C-group files land with it, and gate B verified that exact tree | — |
| E80 | The loop ticks while needs_tick() or the activity panel is open, and opens the terminal with ratatui::try_init | 2026-09-29 | | A transcript grows without any agents.toml change, so no snapshot wakes the loop to show new lines; try_init returns an init failure as an error instead of panicking | — |
| E82 | CLI tests pin HERDR_BIN_PATH to a missing sandbox path; orchestrator gated test-only helpers behind cfg(test) and deleted the unused Herdr::with_bin | 2026-09-29 | | Unset, the tests fall back to the installed herdr and a regressed dry run would reload the live server; removing the allow surfaced five dead_code warnings in diagram/mod.rs, diagram/order.rs, diff.rs, herdr.rs and view/selector.rs | — |
| E84 | README names the traversal view ego (its CLI value); CLAUDE.md says a moved binary needs its hook and keybinding entries fixed by hand | 2026-09-29 | | ViewKind::parse and --view accept ego; setup skips any existing entry naming glimpse, so rerunning it does not repoint a moved binary | — |
| E85 | Minted a task to correct the AgentRecord timestamp doc comment, harvested from a TANGENTIAL line and applied by the orchestrator | 2026-09-29 | | The comment said timestamps come from the hook payload or transcript, but record stamps them with now_rfc3339(); cheap, inside a file this run wrote, and compiled by the final pass | — |
| E100 | Poller sends Event::Flows / Event::FlowMtimes and lists flows on its own thread; the capabilities probe runs after the first failed fetch | 2026-09-29 | | FlowsChanged made the UI thread spawn tomlctl flow list (0.4-0.65 s freeze) on every tasks.toml write; the poller now relists only on a flow-set or context.toml change and otherwise sends mtimes. The eager probe cost a spawn before every first snapshot, and a successful fetch already proves the capability, so it runs only on failure and its message still wins | — |
| E101 | pane::ensure looks up an existing pane unlocked; the split path takes an OS File::try_lock and re-lists before splitting | 2026-09-29 | | Every SubagentStart serialised a read-only herdr pane list behind the global lock, so bursts hit the 3 s wait; and the stale takeover let a slow holder's Drop delete its successor's lock. Double-checked locking keeps the one-split guarantee, and an OS lock is released only by its holder or by the OS when the holder dies | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-09-29 | 101 entries: status-transition × 2, task-completion × 42, deviation × 34, verification × 20, checkpoint × 3 | 07617ce, 0adfde5, 128ed87, 1753880, 22c7a94, 26194f3, 276629b, 2a0b2f5, 2ac5664, 2c3ce5c, 303dec1, 410ff42, 4111eaa, 46ef07c, 4bc83fd, 5374c8a, 558e328, 5858d7b, 5d52046, 76eb415, 79b5b3d, 843d26f, 910b4f3, 951b90e, 9720ff7, 9de79fc, b32333e, b501d48, c348e7a, cb81394, cc60a17, d8d0118, d9774f7, dc15786, e12ceb8, e44aa30, ef5f424, fc43ce3 |

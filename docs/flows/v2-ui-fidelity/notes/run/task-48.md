Status: done — ui-layout.md section 5 now carries the <surface>-<n> naming rule for states and controls

```toml run-note
files_touched = ["claude/skills/plan-v2-engine/references/ui-layout.md"]

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

In ui-layout.md section 5: States gains a rule naming every state `<surface>-<n>`; Controls gains that a control takes its surface's prefix and the next free number, never a name describing the planted defect; the look-entry example states are now `settings-01`, `settings-02`, `settings-04`. The reproduce-status accept check is the gate's.

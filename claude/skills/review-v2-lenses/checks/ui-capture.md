+++
id = "ui-capture"
status = "stable"
families = ["code"]
executor = "verification"
fence = "UI CAPTURE"
activation = "signal"
apply_when = { signals = ["ui-change"] }
discover = [
  "no-signal: when the run's recorded signals do not include `ui-change`, resolve `none — ui-capture: none; the change touches no UI`",
  "declared: the `command` in `.claude/ui-capture.toml` at the repo root, run from the repo root as written with ` --out <run dir>/ui-capture` appended, never with `--only` or `--list`",
  "none — ui-capture: none; the project declares no .claude/ui-capture.toml",
]
+++

# UI capture

## Runs

The project's own capture command, once per run, before the cells, so the UI lens finders judge real captures of the changed UI rather than reading its source alone. The capture contract — `.claude/ui-capture.toml` with its `command`, `out`, `paths` and `controls` keys, the command's `--out`, `--only` and `--list` flags, the per-state `<state>.png` and `<state>.txt` outputs and the `geometry.json` layout report — is the `ui-capture` skill's; this check only runs it.

The surveyor walks `discover` in order and stops at the first rung that settles the line. It runs nothing, and it never runs the capture command or its `--list` to see which states exist. A run whose signals lack `ui-change` resolves `none` on the first rung. Otherwise it reads `.claude/ui-capture.toml` at the repository root: when the file is absent, or holds no `command` string, the line resolves to `none — ui-capture: none; the project declares no .claude/ui-capture.toml`. When it holds one, the resolved command is that `command` with ` --out <run dir>/ui-capture` appended, `<run dir>` being the run's `dir`. The digest lists it on one `verification:` line keyed `ui-capture`.

The resolved command is never narrowed or altered:

- no `--only`, so the command captures every state the project lists, the empty, long, narrow and error extremes included, whichever of them the change reaches;
- no `--list`, which captures nothing;
- the declared `out` is not used: `--out` overrides it, so every capture of the run lands under the run directory and none lands in the project's own output directory;
- the command is never wrapped to retry, to start a dev server, or to install a browser or a dependency it reports missing.

A capture drives a browser, a window or a test backend, so it can take longer than a lint. It runs in the same `verification` dispatch as the other baselines, with the `timeout` the engine gives every baseline command.

## Interpreting the result

The command's exit status and its output directory together say what the finders have:

- **Exit 0.** Every state was captured. Under `<run dir>/ui-capture` each state has a `<state>.png`, a `<state>.txt`, or both, and `geometry.json` is an object keyed by state.
- **Exit 0, but an image state has no geometry key.** The capture ran, but at least one state written as `<state>.png` has no key in `geometry.json`, or `geometry.json` is missing altogether while a `<state>.png` exists. That capture is incomplete, not clean: a vision judge is near chance on clipping and overflow, so a missing measurement must never read as a clean one. A state written only as `<state>.txt` needs no key, since clipping and overflow show in its text.
- **Any other exit.** At least one state was not captured: a capture failure, never a pass. Whatever the directory holds may be partial.
- **No config.** `none — ui-capture: none; …` means no capture ran. The lenses fall back to the changed source and grade their findings as their `No capture` probe says.

To tell exit 0 from incomplete, the orchestrator, when the verification return arrives, lists the `*.png` files directly under `<run dir>/ui-capture` and the top-level keys of `<run dir>/ui-capture/geometry.json`. Each `<state>.png` whose `<state>` is not a key is a state missing its geometry. It also reads the `controls` array of `.claude/ui-capture.toml`, the project's planted-defect states, absent meaning none. Reading those three is all it does: it judges no capture.

## Fence or finding mapping

`pack.md` carries a `UI CAPTURE` fence holding only `see the run's recorded baselines`, because the captures do not exist yet when the surveyor writes the pack. The orchestrator records one `baselines` entry under the key `ui-capture`, the output directory standing where other baselines record a log path:

- exit 0 and every image state keyed: `<run dir>/ui-capture`;
- exit 0 with image states missing a key: `<run dir>/ui-capture — ui-capture: incomplete: <state>, <state>`, naming every state missing it;
- any other exit: `<run dir>/ui-capture — ui-capture: failed, exit <n>; log <log path>`, the log path being the one the verification return gave;
- no config, or no `ui-change` signal: the `none — ui-capture: none; <reason>` line the surveyor resolved, recorded as written.

When the declaration's `controls` is non-empty, each entry that names the directory ends with `; controls: <state>, <state>`, naming every control as declared, so exit 0 records `<run dir>/ui-capture; controls: <state>, ...`.

`review show <cell> --for finder` carries that entry. A finder reads the states in its slice from the directory it names. A state named under `controls:` carries a defect planted on purpose: the finder skips it, reports neither its planted defect nor its `geometry.json` entries, and counts it in its coverage note as a control. `ui-capture: incomplete` means the named states are reported as capture failures, as the lenses' measured-geometry probe says, and the rest are read as captured. `ui-capture: failed` means only what the directory holds may be read, and every state the slice reaches that has no output there is judged from source. `ui-capture: none` means no capture: the lenses read the changed source.

This check writes no findings. A missing geometry key or a failed capture is reported by the finder that reads it, inside its own slice, never by this check.

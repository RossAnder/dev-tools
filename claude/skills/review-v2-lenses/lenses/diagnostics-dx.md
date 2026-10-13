+++
id = "diagnostics-dx"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["error-messages", "log-quality", "help-text"]
activation = "candidate"
slice = "per-subsystem"
v1_category = "testability"
+++

# diagnostics-dx

## Question

When this code fails or is first used, can the person in front of it tell what happened, to which input, and what to do next?

## Owns

`error-messages` — an error a user or operator sees that does not name the operation that failed, the input it failed on, and what was expected against what was found; a message that names the wrong case, drops the underlying cause, or tells the reader to do something that would not fix it.

`log-quality` — log lines whose level does not match what an operator must do about them, or whose content does not let the operator find the failing item: an expected condition logged as an error, a real failure logged at debug or not at all, a line with no identifier for the request, file or record it concerns, or a secret written in the clear.

`help-text` — help, usage, defaults and option descriptions a first-time user cannot discover or would be misled by: an option missing from the help, a default the help does not state or states wrongly, a required argument presented as optional, a usage line that no longer parses.

## Probes — code

Read each surface from the reader's side: who sees it, at what moment, and what they need to act.

- **Error messages.** For every error the change raises, returns or reformats on a path a user or operator reaches, check that the text names the operation, the input (path, id, key, value) and the expected versus actual condition. Follow wrapped errors to the top-level printer and confirm the cause chain survives: a context string that replaces the underlying error rather than wrapping it loses the reason.
- A message whose wording describes a different failure from the branch that emits it, or that is shared by two branches a reader needs to tell apart.
- An error that tells the user to run a command, set a flag or edit a file: confirm the command parses, the flag exists and the remedy would clear the error.
- **Logs.** For each log call the change adds or alters, judge the level against the action it demands: error for a failure someone must fix, warning for a degraded state, info for a milestone, debug for detail. Check the line carries the identifier an operator would search for, and that it does not log credentials, tokens or whole request bodies.
- A failure that is caught and handled with no log on a path the project already logs, so the only trace of it is a later, unrelated symptom.
- **Help text.** For each flag, option, command or config key the change adds or alters, read the help output, usage line and documented default as a first-time user would. Compare each stated default and each enumerated choice with the value the code actually uses.
- Exit codes and machine-readable error shapes the change alters that scripts or callers parse.

## Does not flag

- Confirmations: a message, log line or help entry that reads well is a coverage-note line, never a row.
- Debug-log gaps on paths the project does not already log, and log verbosity preferences with no operator consequence.
- DX polish on internal-only surfaces: messages only a developer of the module itself sees, panics on states the types make unreachable, test-helper output.
- Wording and tone preferences that leave the message's meaning and its remedy intact.
- A message format a comment, decision record or project rule fixes deliberately, such as a stable error string callers match on; challenge it as advisory if it still misleads.
- A doc page or README that disagrees with behaviour; prose outside the program's own output belongs to doc-drift.
- Swallowed errors and wrong behaviour on the error path; correctness owns those. This lens judges what the reader is told, not whether the failure is handled.
- Out-of-scope surfaces, and a message rewrite whose gain is negligible against breaking callers that match on the old text.

## Edit forms

- `change` — rewrite a message, log line, level, help string or stated default. `old` is the exact current text; `edit` gives the replacement naming the operation, input and expected versus actual, or the corrected default.
- `add` — a missing help entry, an omitted default in the help, or the input identifier a log line or error lacks. `old` is the adjacent anchor text.
- `remove` — a log line that duplicates another at the same point, or a credential written to a log. `old` is the exact line.

## Advisory angles

- `direction` — an error-reporting or logging convention the subsystem lacks, such as structured fields or an error-code scheme, where adopting it touches many sites.
- `challenge` — a deliberately fixed message or exit code that misleads its readers.

## Severity calibration

- `critical` — a secret, token or personal data written to a log or error output.
- `major` — an error or help text that sends the user toward a wrong or destructive remedy, a stated default that differs from the real one on an option with side effects, or a failure logged nowhere on a path the project logs.
- `minor` — an error missing the input or the expected value, a mis-levelled log line, or an undocumented option with a harmless default.

+++
id = "security"
status = "stable"
families = ["code", "plan"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["input-validation", "injection", "access-control", "secrets-and-crypto"]
activation = "signal"
apply_when = { signals = ["trust-boundary", "dependency-change"] }
slice = "per-subsystem"
methods = ["trace"]
v1_category = "security"
research_domain = "security"
+++

# Security

## Question

Where does data from outside the process's trust enter this scope, what does it reach, and is the check that boundary needs present on the way?

## Owns

`input-validation` — untrusted data (user input, files, network, environment, another process's output) reaching a sink without the check the boundary needs: a size or count bound, a path confined to its root, a type or range check, a parser configured for hostile input.

`injection` — untrusted data reaching a query, a shell command line, a filesystem path, a template or a regex as code rather than as data, through concatenation or formatting where a parameter, an argument vector or an escape belongs.

`access-control` — authentication or authorisation missing, bypassable or checked against the wrong principal at a boundary: an endpoint, a command, a file permission, a token's scope.

`secrets-and-crypto` — secrets stored, logged, committed or sent in the clear, and cryptography that is weak, home-made or misused: a fixed nonce, a non-constant-time comparison, a disabled certificate check, a predictable token.

## Probes — code

Work from the boundary inward. For each entry point in the slice, name it, name what the data reaches, and read every function between the two before claiming a check is missing; a check one call further down is the Counter line of the finding.

- What enters: request bodies and headers, query strings, CLI arguments, files the process opens on a caller's say-so, environment variables, stdin, child-process output, deserialised messages, and the contents of a repository the tool was pointed at.
- What it reaches: a SQL or query-builder string, a shell or process command line, a path join, a template or HTML render, a regex compiled from input, a deserialiser that picks types, an allocation sized by input, a redirect target.
- Paths: a joined path that can escape its root through `..`, an absolute component, a drive letter, a UNC prefix or a symlink; a temp file created at a predictable name.
- Commands: an argument built by string formatting and handed to a shell, rather than passed as a separate argument; an executable resolved through a search path the caller controls.
- Bounds: a read, a loop or an allocation whose size the input chooses, with no cap.
- Access: a new endpoint or command missing the check its siblings carry; an authorisation decision made from a client-supplied field; an identifier from the request trusted to select another principal's data.
- Secrets: a token, key or password written to a log, an error message, a panic, a committed file or a URL; a credential compared with ordinary equality; a secret read into a type that prints itself.
- A dependency the change adds or bumps: what untrusted data the code now hands it, and whether the call site configures it for that data — entity expansion on an XML parser, polymorphic type handling on a deserialiser, certificate verification on a client. Advisories against the dependency itself belong to `external-validity`.

Name the weakness with its most specific CWE id in the finding's evidence.

## Probes — plan

- Every entry point a task adds — an endpoint, a verb, a file the tool reads on request, a hook, a message handler — and whether any task validates what it accepts, bounds its size and confines its paths.
- Every task that builds a query, a command line, a path or a template from input, and whether its Action or Detail fixes the parameterised or argument-vector form, or leaves the choice to the executor.
- A new capability with no task deciding who may use it, or an authorisation check the plan places in the client.
- Tokens, keys and credentials the plan stores, logs, caches or passes between processes, and whether a task fixes where they live and who can read them.
- A plan that moves a trust boundary — a local tool exposed on a port, a file format accepted from other users, a privileged step run on another's input — with no task naming the new boundary.

## Does not flag

- Confirmations: a boundary checked and found sound goes in the coverage note, never in a row.
- A check claimed missing on the strength of a comment or a function's name; read the code the data passes through.
- A choice a comment, an ADR or a CLAUDE.md line documents as accepted risk; that is `advisory` with `angle = challenge`, never a defect.
- Threat-model dumps, defence-in-depth wish lists, hardening with no vector, and theoretical items: a finding names a plausible vector in the actual deployment, from a source the attacker controls to a sink that harms.
- Data that never crosses a trust boundary — the process's own constants, its own config written by its own installer, input from the same user who runs the tool on their own machine — unless the scope shows it does cross one.
- Advisory lookups and whole-lockfile sweeps; `external-validity` owns advisories, and the project's audit command owns the full tree.
- Anything outside the slice's boundaries, and anything the `LINT BASELINE` already reports.
- A fix whose cost and risk outweigh a narrow, unlikely vector; record it as `advisory` with `angle = risk` if it matters at all.
- More than five defects in one cell: keep the five with the most direct vector and fold the rest into the coverage note.

## Edit forms

Security writes defects, not cleanup: a missing or wrong check changes behaviour at a boundary, so its fix is a `fix`, never an `edit`. The one cleanup form is `remove` — a secret, a credential or a private key committed in a test fixture, an example or a comment, where nothing reads it; the advisory to rotate it goes beside the row. Its `old` quotes only the non-secret part of the line, such as the key name and the `=`, never the value, and its `edit` and `summary` describe the value without repeating it: the store is committed and exported, so a quoted secret is republished.

## Advisory angles

- `risk` — a weakness whose vector depends on a deployment fact the cell cannot see, such as whether a port is exposed or a file is shared; name the fact.
- `challenge` — a documented decision that accepts a security cost; name the decision in `challenges`.
- `alternative` — a safer library or construct for a pattern repeated across the scope, when adopting it needs a decision.

## Severity calibration

- `critical` — a vector reachable from an untrusted source today that executes code, reads or writes outside its root, bypasses an authorisation check, or discloses a live secret.
- `major` — a real weakness with a nameable vector that needs one precondition the deployment makes plausible.
- `minor` — a real weakness with a narrow vector: a local-only tool, a precondition the attacker rarely controls, a bounded disclosure.

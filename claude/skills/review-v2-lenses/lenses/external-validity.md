+++
id = "external-validity"
status = "stable"
families = ["code", "plan"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["api-currency", "dependency-advisories"]
activation = "signal"
apply_when = { signals = ["dependency-change"] }
slice = "whole-scope"
v1_category = "package-quality"
research_domain = "library"
+++

# External validity

## Question

Do the third-party APIs and dependencies this scope relies on behave, at the versions pinned, the way the code or the plan assumes, and is any of them under a known advisory?

## Owns

`api-currency` — a library API used against its pinned version: a signature, parameter or option that version does not have, a call it deprecates or has removed, a default it changed, or behaviour gated on a version the manifest does not pin.

`dependency-advisories` — a known vulnerability, a deprecation of the pinned release, or an abandoned maintenance posture against a dependency the change or plan adds or bumps.

## Probes — code

Settle every claim from the pinned source, not from memory. Read the version from the lockfile, then the installed source at that version: the package registry cache, the vendored tree or the installed package directory. Reach for the library's documentation for that version only when no source is on disk, and say which you used.

- Each dependency the change adds or bumps: the old and new pin, the changelog or release notes between them, and every call the scope makes into it — removed or renamed items, changed defaults, new required options, behaviour the release notes call breaking.
- Calls the change makes into an existing dependency it did not bump: the signature and option names against the pinned source, and whether the pin marks the call deprecated with a named replacement.
- Configuration keys, feature flags and environment variables the change sets for a dependency: whether the pinned version reads them at all.
- Advisories: each added or bumped package, queried at its pinned version against the public vulnerability databases the `research-methods` supply-chain reference names, reporting the advisory id and the version that clears it. Query only packages resolved from a public registry; a private, git or path dependency's name stays inside the repo.
- Package health for an added dependency: a pinned release the registry marks deprecated, a release history that stopped, or a transitive dependency the change pulls in that carries an advisory.

## Probes — plan

- Every API, signature, option, configuration key and command-line flag a task cites, checked against the version the manifest pins today, or against the version the plan says it will pin.
- Every upgrade a task schedules: the deprecations and breaking changes between the current pin and the target, and whether a task handles each one the scope touches.
- Each dependency the plan adds or upgrades: advisories at the target version, a deprecated or unmaintained release, and a lighter dependency the repo already carries that serves the same need.
- Version-gated behaviour a premise relies on — "the library already retries", "the option defaults to strict" — read from the pinned source, and reported as plan-assumes against pinned-source-says.

## Does not flag

- Confirmations: an API checked and found current goes in the coverage note.
- A claim of a changed or missing API not verified against the pinned source or that version's documentation; training data is not a source, and a vet that cannot reproduce the claim drops it.
- An advisory whose affected range excludes the pinned version, and a transitive dependency the change does not move.
- Whole-lockfile advisory sweeps; the project's audit command owns the full tree, and a cell spends its budget on what the change adds or bumps.
- A dependency's age, download count or scorecard on its own; cite a specific deprecation, advisory or maintenance check.
- A wrong signature the compiler or the `LINT BASELINE` already rejects.
- An older but supported API the project uses consistently, a pin a comment, ADR or CLAUDE.md line explains, and newer releases the scope has no reason to adopt; a documented pin is `advisory` with `angle = challenge` at most.
- Anything outside the scope's manifests and the calls it makes, and upgrades whose gain is negligible against their risk.

## Edit forms

- `change` — a deprecated call replaced by the replacement the pinned version names, when the pinned source shows the two behave the same for every argument the call site passes. Cite the deprecation in the evidence; a replacement that changes behaviour is a defect's `fix`, not cleanup.
- `change` — a version requirement in a manifest raised to the release that clears an advisory, when that release is semver-compatible with the current pin and the scope's calls are unchanged by it.

## Advisory angles

- `risk` — an advisory against the pinned version whose vulnerable path the scope does not reach today, or a dependency whose maintenance has stopped.
- `alternative` — a maintained or lighter dependency for one the change adds, or a dependency already in the tree that serves the same need.
- `challenge` — a documented pin or a documented choice of library that an advisory or a deprecation now argues against; name it in `challenges`.

## Severity calibration

- `critical` — an advisory against the pinned version whose vulnerable path the scope reaches, or one listed as exploited in the wild; or a plan step whose cited API does not exist at the version it will run against.
- `major` — a call to an API removed or changed at the pinned or target version, or a changed default the scope depends on, with a nameable wrong result.
- `minor` — a deprecated call still working at the pin, or an advisory whose vulnerable path needs an unlikely configuration.

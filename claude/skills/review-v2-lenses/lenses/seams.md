+++
id = "seams"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["hidden-dependencies", "hard-wired-concretes"]
activation = "candidate"
slice = "per-subsystem"
v1_category = "testability"
+++

# seams

## Question

Where does the changed code reach for time, randomness, the environment, IO or a concrete collaborator that a test cannot substitute, so the behaviour that matters cannot be tested deterministically?

## Owns

`hidden-dependencies` — globals, process-wide state, the clock, randomness, environment variables, the working directory, the filesystem and the network reached directly from logic, with no parameter or injection point a test can control.

`hard-wired-concretes` — a concrete collaborator constructed inline (a client, a store, a process runner, a logger with side effects) where a test needs a substitute to exercise the logic in isolation.

## Probes — code

- For each branch the change adds, ask what a test would need to control to reach it. If the answer is the time, a random value, an environment variable or a file the logic opens itself, the dependency is hidden.
- Logic that reads the clock or generates ids inline and then branches or formats on the result, so its output is not reproducible.
- Process-wide singletons, mutable statics and caches the change reads or writes, which make tests order-dependent or force them to run serially.
- A function that both decides and performs IO, where the decision could take the IO's result as an argument and stay pure.
- Collaborators constructed inside the function under test rather than passed in, where the test for the changed behaviour would otherwise need a real network, database, process or terminal.
- Paths built from the current directory or the user's home inside logic, so a test cannot point them at a temp directory.
- A seam that exists but the change bypasses: the code takes an injected clock or store and the new line calls the concrete one directly.

## Does not flag

- Confirmations: code that already takes its dependencies as arguments is a coverage-note line, never a row.
- Composition roots and entry points — `main`, startup wiring, the CLI dispatch layer — where constructing concretes is their job.
- Trivial accessors and thin adapters whose only content is the IO call itself; there is no logic to isolate.
- Dependencies the project deliberately leaves concrete, recorded in a comment, ADR or project rule, and coupling confined to one module that its own tests already exercise through the real dependency cheaply (a temp directory, an in-memory database).
- Speculative seams for substitutions no test needs today; adding an interface with one implementation and no test using a second is not a finding.
- Missing tests themselves; test-gaps owns those, and a gap a seam would unblock is cross-referenced there.
- Extension points for other callers (plugin traits, registries); whether those are wired belongs to ripple, and whether they are well shaped to architecture.
- Out-of-scope paths, generated code, and style-only preferences about how a dependency is passed.

## Edit forms

- `change` — pass the dependency in: take the clock, the random source, the path or the collaborator as a parameter or field instead of reaching for it, keeping the existing call sites' behaviour by passing the concrete value there. `old` is the exact line that reaches for it.
- `change` — route a new line through a seam the module already has, when the change bypassed it.
- A seam that needs a new trait or interface across module boundaries is advisory with `angle = alternative`, not cleanup.

## Advisory angles

- `alternative` — introducing an abstraction, such as a clock or filesystem trait, that several modules would share.
- `risk` — process-wide state that makes the suite order-dependent but whose removal needs a design decision.

## Severity calibration

- `critical` — none expected; a hidden dependency is a testability cost, not a failure, unless it also makes production behaviour wrong, which correctness owns.
- `major` — a hidden dependency that makes a test of the changed behaviour impossible or non-deterministic, such as a branch on wall-clock time with no injection point.
- `minor` — a hard-wired collaborator that forces a test onto a slower real dependency but still lets it run deterministically.

# Acceptance probing

How a plan's **Acceptance** criteria are written, labelled and probed before the plan ships.
`/plan-new` Phase 7 and `/review-plan` Step 2.6 run this method; the format rules carry only the
two-control rule's statement and point here.

## Acceptance sub-bullets

Write one sub-bullet per criterion under **Acceptance**, each opening with its polarity:

- `forward:` — describes the tree after the task lands. Append `today: <value>` with the
  negative-control baseline, so the executing agent inherits the before-value.
- `falsifier:` — describes the tree before the task lands; the negative control settles it.
- `guard:` — a regression guard, a forward criterion that already holds today. It is the
  `(regression guard)` tag's other spelling, and both are accepted. A guard never stands
  alone: pair it with a `forward:` or `falsifier:` criterion that discriminates.

A task with a single criterion may keep it inline on the **Acceptance** line.

```markdown
- **Acceptance**:
  - forward: `grep -c '^## Summary' docs/plans/x.md` prints `1` (today: `0`).
  - guard: `cargo test --manifest-path tomlctl/Cargo.toml --test tasks_read` passes.
```

## Two-control rule

An acceptance command ships only after it has been run twice. Any criterion that is a read-only
shell command (`grep`, `awk`, `sed`, `rg`, `wc`, `jq`, `git diff | …`) is probed before the plan
is written — re-reading is no substitute, because its defects are *tool-semantics* defects that
read as correct on the page. Carriers run both probes with the
[acceptance probe helper](#acceptance-probe-helper) below.

**Label each criterion's polarity before probing it.** A *forward* criterion describes the tree AFTER the task lands ("the section counts 3", "the diff is comment-only"). A *falsifier* criterion describes the tree BEFORE it ("the suite fails on the old values", "this goes red until task N", "the derive command returns 3 today"). Both are probed by the same call, but the verdicts invert: a forward criterion must FAIL the negative control, a falsifier criterion must PASS it. Reading a falsifier with forward polarity is how a permanently-green acceptance is certified healthy.

- **Negative control** — run it against the current tree. A *forward* criterion that already holds discriminates nothing for its task; otherwise record the baseline on the **Acceptance** line (`today: 0`) so the executing agent inherits the before-value. The one legitimate forward baseline pass is a regression guard ("test X still passes") — tag it `(regression guard)` and pair it with a criterion that does discriminate; a guard alone cannot tell success from a no-op. A *falsifier* criterion is a claim about the present, so this probe settles it outright and no positive control is needed: if it does not hold now, the check it names does not bind what the task changes.
- **Positive control** (forward criteria) — run the same pipeline against the input a *correct* implementation would produce: `sed -n` the real region the task edits, apply the described edit by hand, and pipe that in place of the file read (for a `git diff` criterion, `printf` the hunk the edit yields). If the criterion still fails, **no correct implementation can pass it** — the command is broken, not strict. When the criterion is a test assertion rather than a command, the positive control is a read: an acceptance that quantifies over a set ("every card carries a fade") must cite the existing test or code that fixes the set's real shape — the counter-example is usually already asserted in a suite the plan never opened — and assert over the subset it leaves.

**An acceptance that delegates to a named test carries a falsifier by default.** "Test X passes" binds nothing on its own — state the perturbation that makes X red (the old value, the reverted line, the removed guard) and probe *that*, since the pre-change tree already contains it. A suite asserting only ordering passes on the old triple and the new one alike: mechanically verifiable, permanently green, and indistinguishable from a healthy acceptance until someone runs it. Probe with the narrowest available form (`--test <name>`, `-E 'test(<area>)'`); when only a whole-suite run would settle it, label the line **predicted, unverified** rather than guessing.

Traps confirmed in the wild, each reading as correct: `awk '/^## X/,/^## /'` yields one line, because a range start also tests the end pattern on its own record and `## X` matches `^## `; a `git diff … | grep -vE '^[+-]\s*(\*|/\*|//)'` comment-only filter rejects every correct edit to a file whose block comments use bare prose continuations; `grep visiblebox` misses the two-word "visible box" actually in the file.

## Acceptance probe helper

Run by `/plan-new` Phase 7 and `/review-plan` Step 2.6, in the orchestrator's own shell as **one** batched call for the whole plan: never a call per task, and never through the `verification` agent, which stops at the first non-zero exit while a healthy negative control exits 1.

```bash
p(){ e=$(mktemp); o=$(eval "$3" 2>"$e" </dev/null); r=$?; printf '%s\t%s\trc=%s\t%s\t%s\n' "$1" "$2" "$r" "$(printf %s "$o" | tr '\n\t' '| ' | cut -c1-100)" "$(tr '\n\t' '| ' <"$e" | cut -c1-100)"; rm -f "$e"; }
p 19 neg "awk '/^## The shell/,/^## /' docs/a11y-ledger.md | grep -c 2.3.3"
p 19 pos "printf '## The shell\n- SC 2.3.3 met\n## Next\n' | awk '/^## The shell/,/^## /' | grep -c 2.3.3"
p 9 neg 'git diff -U0 src/rail.ts | grep -E "^[+-][^+-]" | grep -vE "^[+-]\s*(\*|/\*|//)"'
p 9 pos 'printf "+/* Why:\n+   bare prose\n+ */\n" | grep -E "^[+-][^+-]" | grep -vE "^[+-]\s*(\*|/\*|//)"'
```

Quote each command with the quote style it does not itself contain; inside double quotes a literal `$` is `\$` — the quotes are the *caller's*, so an unescaped `awk '{print $1}'` expands `$1` against the calling shell and almost always yields the empty string, silently turning the probe into a different program rather than erroring. Prefix `cd <dir> &&` when the **Acceptance** line names a working directory — the subshell discards it. Every probe prints one tab-separated line — `task`, `neg|pos`, `rc`, stdout, stderr (each stream's first 100 bytes, newlines folded to `|`) — and nothing short-circuits. Judge on the stdout and stderr columns, not the rc: a pipeline's exit status is its last stage's, so an `awk: fatal` first stage still reports `rc=1` like a healthy miss, and git's `LF will be replaced by CRLF` warning lands in the stderr column without touching the verdict.

## Verdicts

| polarity | negative control | positive control | verdict |
| --- | --- | --- | --- |
| forward | already holds | — | **vacuous** — rewrite, or mark it a guard (a `guard:` sub-bullet or the `(regression guard)` tag) and add a discriminating criterion |
| forward | does not hold | holds | **healthy** — record the baseline on the **Acceptance** line (`today: 0`) |
| forward | does not hold | does not hold | **unsatisfiable** — the command is broken; fix the command, never the task |
| falsifier | holds | — | **healthy** — the named check does bind what the task changes |
| falsifier | does not hold | — | **permanently green** — the check cannot detect this task's change; strengthen the assertion or name one that binds the changed values |
| either | stderr carries `No such file`, `command not found`, or `fatal` | any | **broken** — wrong path, tool, or working directory |

Skip only commands that write build output (`target/`, `dist/`) — both carriers run before approval. A criterion that cannot be probed read-only is labelled **predicted, unverified** on its own line.

---
name: backlog-capture
description: "Captures a real but out-of-scope discovery into the repo-scoped store at `.claude/backlog.toml` via the `tomlctl backlog` group — when minting is warranted and when it is not, the mandatory `backlog check` gate and how to act on each verdict (duplicate, previously-resolved, duplicate-id, likely-duplicate, related, novel), the kind and status vocabularies, the orchestrator-only writer rule and the `TANGENTIAL:` heading a sub-agent uses to surface a candidate instead, the git-ignored evidence drop-box and its publication discipline in a public repository, and the capture idioms. Use whenever an agent or orchestrator notices a real but out-of-scope issue — a flaky test, a bug elsewhere, a follow-up, an annoyance — and before any `tomlctl backlog add`."
---

# Backlog capture

`.claude/backlog.toml` is a repo-scoped capture log: the place a discovery goes when it is
real, worth keeping, and not what you were sent to do. It is not a tracker and not a plan.
Its job is that the next agent to trip over the same thing finds a row saying *we know, here
is how to work around it* instead of rediscovering it.

This skill owns the capture **discipline**. The full flag surface of the `tomlctl backlog`
group lives in `claude/skills/tomlctl/references/backlog.md`; do not go looking for a flag
table here.

## Mint when, and when not

Mint when all three hold:

- **Real** — you observed it, not inferred it. A failing test you saw, a wrong result, a
  path that cost you ten minutes.
- **Out of scope** — fixing it now would widen the task you were given. In scope means fix
  it; the backlog is not a substitute for doing the work.
- **Durable** — it will still be true tomorrow, and someone landing on the same code will
  hit it.

Do **not** mint:

- Anything you are about to fix in this same change.
- A restatement of an open item in a review or optimise ledger, a plan task, or a lumina
  work item — those stores own their own findings, and a copy here rots independently.
- A hunch, a style preference with no consequence, or "we could one day". `direction` is for
  a decision someone has actually argued for, not a daydream.
- A secret, a customer name, or a stack trace quoting paths outside the repository. See
  **What publication means** below — everything you write here ships to every clone.

## The `backlog check` gate

**Never mint blind.** Run `backlog check` first, every time, and act on the verdict:

| Verdict | What it means | What to do |
|---|---|---|
| `duplicate` | Exact fingerprint match on an existing row | Do not mint. `add` would only bump `seen_count`. Read the row's `context` and use it. |
| `previously-resolved` | Fingerprint matches a row aged into `[[compacted]]` | Do not mint. Read the compacted row's `context` — this was decided once already. Mint only if you can say why the decision no longer holds. |
| `duplicate-id` | Two stored rows share one id | A merge artefact. Surface it to the user; do not paper over it with a fresh row. |
| `likely-duplicate` | Summaries are near-identical by character trigram | Read the named candidate in full. Mint only if yours is genuinely a different problem. |
| `related` | Shares wording, an area prefix, or tags | Mint, and pass `--related <the candidate id>` so the edge exists. |
| `novel` | Nothing close | Mint. |

`check` is read-only and a missing store answers `novel`, so the first capture in a repo
needs no setup.

### The fingerprint includes kind and area

An item's id is `B-` plus the leading hex of `dedup_id = sha256(kind | area |
normalise(summary))`. Two consequences:

- **Give `check` the same `--kind` and `--area` the following mint will use.** Change either
  between the two calls and you have probed a different fingerprint than you are about to
  write — the gate returns `novel` and the mint lands a second row for a known issue. This is
  the single most common way to defeat the gate.
- Because the derivation is content-only, the same discovery minted from another worktree,
  another branch, or another machine lands on the same id. Ids are stable, not allocated.

A rephrased summary is a different fingerprint. If you mean the same issue, reuse the wording
the store already has rather than improving it.

## Vocabularies

**`kind`** — `bug`, `flaky-test`, `debt`, `direction`, `annoyance`, `question`, `other`.
Anything unrecognised is coerced to `other` with a warning rather than rejected, so a typo
mints a real but badly-filed row; spell it deliberately.

**`status`** — `open` on mint, then `promoted` (picked up by a plan or flow), `dismissed`
(decided against), or `resolved` (fixed). Status is moved only by `backlog triage`, never by
re-minting, and `add` never rewrites the `status` or `summary` of a row it lands on.

## Who writes

**Sub-agents never write to the store.** Implementers, research agents and verification
agents do not run `backlog add`, `relate`, `triage`, `compact` or `evidence dir`, and do not
create evidence directories. Read-only `backlog check`, `show` and `list` are allowed when they
help decide whether something is already known. They surface candidates in their return payload under a fixed heading:

```
TANGENTIAL: <kind> | <area> | <summary> | <why it matters>
```

One line per candidate, and `none` when there are none. The heading is fixed so the
orchestrator can find it mechanically.

**The orchestrator is the only writer.** On receiving a `TANGENTIAL:` line it runs
`backlog check`, then mints the ones the verdict allows, tagging provenance with
`--origin <the command>` and `--flow <the flow slug>`. Concentrating writes in one place is
what keeps parallel sub-agents from racing on one TOML file and its integrity sidecar.

## Minting

The field that earns the item its keep is `--context`: how to work around the issue, or what
the next reader should do first. A `check` hit whose `context` is empty tells the next agent
only that someone else was here too.

`--on-duplicate bump` is the default and the right one: on a fingerprint collision it
increments `seen_count`, refreshes `last_seen`, and unions `tags` and `evidence`, leaving
`summary` and `status` untouched. A rising `seen_count` is the store's own signal that an
item deserves promoting.

`--evidence` takes either a `path:line` pointer into tracked source, or a bare filename
inside the item's own evidence directory. Nothing else.

## Evidence directories

Each item may have a drop-box at `.claude/backlog-evidence/<item-id>/`. Its contents are
git-ignored; a four-line `.evidence` marker is tracked, and is what makes the directory
survive into a fresh clone once the files have been left behind.

**Never hand-derive the path.** Ids widen from 8 to 10 to 12 hex on collision, so a directory
built from an eyeballed prefix is owned by nothing — `evidence audit` reports it `unowned`
and it is invisible to `show` and to `list --has-evidence`. Ask for the path:

Run `tomlctl backlog evidence dir <id>`, which resolves the id against the store, creates the
directory and marker if absent, and prints the path. Copy into exactly the path it printed.

**Name the file for what it shows.** A manual `cp` carries no caption; the filename is the
only one it will ever have. `pty-spawn-error-5.png`, not `Screenshot 2026-09-02 141233.png`.
Where a filename clarifies a sentence, reference it by that bare name in the item's `context`
prose — the audit follows those references and reports a `referenced-missing` when the file
is gone.

Nothing in the store enumerates evidence files. `show` reads the directory live: `null` means
no directory was ever created, `files: []` means one exists but its contents are not in this
clone, and a populated list is the files as they are on this machine right now.

`evidence audit` classifies findings as `unowned`, `no-marker`, `oversize`,
`disallowed-extension`, `referenced-missing`, `tracked`, or `empty` (plus `git-unavailable`
when tracked-ness cannot be determined). `--strict` exits non-zero on the first five only:
a `tracked` file is a deliberate publication and an `empty` directory is the normal state in
a fresh clone. Allowed extensions are `png`, `jpg`, `jpeg`, `gif`, `webp`, `svg`, `txt`,
`log`, `json`, `har`, `csv`, `md`, `patch`, `diff`; the default oversize threshold is 2 MiB.
Both are advisory constants that only `audit` consults — no write path enforces them, so the
audit is the only thing standing between a stray `.pem` and a reviewer.

### Publishing a file is a deliberate act

This repository is public, and the drop-box is git-ignored precisely so that a screenshot
cannot be published by reflex. Publishing one is `git add -f <file>` — a human decision,
taken after reading the file for credentials, personal data, session tokens, and a visible
username in a captured path. A HAR or a network `.json` dump carries `Authorization` headers
verbatim and should essentially never be published.

The consequence of leaving a file ignored is that it does not exist in any other clone. So
the item's `context` prose has to carry the finding on its own. The picture is a corroborating
detail for whoever has it; it is never the record.

## What publication means for the store itself

`.claude/backlog.toml` is tracked in a public repository. Every `summary`, `context`, `tag`
and `evidence` string you write ships to every clone, forever, and stays in git history after
any later edit. So: no secrets, no tokens, no customer or personal data, and no verbatim
error output quoting filesystem paths outside the repository. Describe the failure in your
own words instead of pasting the log. `add` prints a stderr advisory when a `summary`,
`context`, `tag` or `evidence` string carries a credential-shaped token or a machine-local
path, but it never blocks the write — the rule above is what governs, and the advisory only
tells you to look.

## After the mint

Items are triaged, not deleted. `backlog triage` moves an item to `promoted` (with `--to`
naming the flow or plan that took it), `dismissed` (`--reason`), or `resolved`
(`--resolution`), and `--reopen` (`--rationale`) puts one back. `backlog compact` ages decided
items into `[[compacted]]`; `open` items are never compacted regardless of age, which is why
a dead item should be dismissed rather than left to rot.

## Idioms

Probe before minting — same `--kind` and `--area` the mint will use:

```bash
tomlctl backlog check --summary "conpty spawn intermittently fails with CreateProcessW error 5" --kind flaky-test --area lumina/server/src/pty --tag pty
```

Mint, with provenance and a workaround:

```bash
tomlctl backlog add --summary "conpty spawn intermittently fails with CreateProcessW error 5" --kind flaky-test --area lumina/server/src/pty --tag pty --context "Empty PATH entry in HKLM; set LUMINA_CLAUDE_BIN to an absolute path to work around it." --evidence "lumina/server/src/pty/spawn.rs:214" --related B-1a2b3c4d --origin /implement --flow lumina-pty-hardening
```

Ask for the drop-box, then copy into the path it prints:

```bash
tomlctl backlog evidence dir B-1a2b3c4d
```

```bash
cp ./capture.png .claude/backlog-evidence/B-1a2b3c4d/conpty-error-5-console.png
```

Read one item with its relations and its live evidence listing:

```bash
tomlctl backlog show B-1a2b3c4d
```

Survey what is open under an area:

```bash
tomlctl backlog list --open --area-prefix lumina/server
```

Hand an item to a flow that is picking it up:

```bash
tomlctl backlog triage B-1a2b3c4d --promote --to lumina-pty-hardening
```

Check evidence hygiene before a commit that touches the drop-box:

```bash
tomlctl backlog evidence audit --strict
```

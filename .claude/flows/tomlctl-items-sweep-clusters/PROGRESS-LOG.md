<!-- Generated from execution-record.toml. Do not edit by hand. -->

# tomlctl pattern-finding support — `sweep`, `items sweep`, `items clusters`, `items orphans` over instances — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | scaffold-the-new-modules | 2026-09-18 | | 7 files |
| E3 | parse-fileline-and-filesymbol-anchors | 2026-09-18 | | 1 file |
| E4 | expose-the-positional-kahn-core | 2026-09-18 | | 2 files |
| E5 | extract-union-find-into-a-shared-module | 2026-09-18 | | 2 files |
| E6 | add-the-bytes-regex-compile-helper | 2026-09-18 | | 1 file |
| E8 | enumerate-tracked-files-through-git | 2026-09-18 | | 1 file |
| E18 | report-unresolvable-instances-as-orphans | 2026-09-18 | | 1 file |
| E19 | build-the-sweep-engine | 2026-09-18 | | 4 files |
| E20 | compute-file-disjoint-clusters-and-dependency-batches | 2026-09-18 | | 1 file |
| E21 | bind-the-engine-to-ledger-items | 2026-09-18 | | 2 files |
| E23 | wire-the-verbs-into-clap-and-dispatch | 2026-09-18 | | 2 files |
| E26 | register-the-verbs-in-the-capability-surface-readme-and-version | 2026-09-18 | | 4 files |
| E27 | dedupe-the-implicit-anchor-in-the-items-sweep-kept-set | 2026-09-18 | | 1 file |
| E28 | integration-test-the-sweep-verbs | 2026-09-18 | | 2 files |
| E29 | document-the-write-path-and-integrity-matrix | 2026-09-18 | | 2 files |
| E30 | document-the-verbs-in-the-tomlctl-skill | 2026-09-18 | | 2 files |
| E31 | point-the-research-method-at-the-standalone-sweep | 2026-09-18 | | 1 file |
| E32 | rewire-the-vet-disposition-sweep-and-ledger-schema-skills | 2026-09-18 | | 3 files |
| E34 | rewire-the-apply-flow-skills-to-the-verbs | 2026-09-18 | | 3 files |
| E36 | update-the-schema-hardcoding-callouts | 2026-09-18 | | 3 files |
| E37 | integration-test-clusters-and-instance-orphans | 2026-09-18 | | 1 file |
| E45 | gate-the-readme-capabilities-sample-version-against-cargotoml | 2026-09-18 | | 1 file |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E7 | Added a third unit test (compile_user_bytes_regex_matches_line_end_before_crlf) beyond the two the acceptance names | 2026-09-18 | | The CRLF line-end property is what the sweep engine depends on and had no other guard; the filter now runs three tests, all passing | — |
| E9 | A malformed --exclude glob is a Validation error rather than silently dropped; containment joins root and entry component-wise | 2026-09-18 | | Exclusions are what stop a sweep matching its own ledger record, so losing one silently would make every pattern hit itself; a canonical Windows root carries a verbatim prefix under which / is not a separator, so a plain join fails to canonicalise at the leaf and the containment check would fall back to the parent | — |
| E12 | Fixed a pre-existing red test on main (check.rs cap probe at 257 rows after the 512 cap bump) directly, committed standalone ahead of the checkpoint A train | 2026-09-18 | `434bc3c` | Commit 7a751f4 raised MAX_NODES to 512 and updated graph.rs tests but not the check.rs probe; the gate cannot go green without it, the fix is one constant, and committing it separately keeps it unattributed to any plan task | — |
| E24 | Live items sweep --update goes through mutate_doc_conditional rather than mutate_doc_plan, and the read-only path relies on read_doc not-found tagging instead of an explicit exists() guard | 2026-09-18 | | mutate_doc_plan always persists after closure success, so a no-change sweep would rewrite the ledger and bump the sidecar; mutate_doc_conditional returns Ok(false) on an empty plan.updated and is the helper items add --dedupe-by already uses; read_doc tags kind=not_found itself and never seeds, confirmed by smoke | — |
| E25 | Minted task 21 to dedupe the implicit file:symbol anchor in items sweep kept output | 2026-09-18 | | Task 11 smoke found kept lists the implicit anchor twice when instances already names it, so every --update appends a duplicate and the no-op path never fires; the fix is one file the run already touched and the existing suite exercises it | — |
| E33 | Vet step 4 names the standalone tomlctl sweep as its primary and items sweep as the post-append re-check | 2026-09-18 | | Both carriers vet before the ledger append (review Step 2.5 interim checkpoint; optimise vet-first-persist-second; plan-new has no ledger), so at vet time no ledger row exists for items sweep to read; the standalone verb takes the sweep strings directly and is read-only by construction | — |
| E35 | Apply Step 3 gains an orchestrator rule that unions the pre-analysis new sites into the verb clusters | 2026-09-18 | | items clusters reads only the ledger recorded instances and the pre-analysis re-sweep is read-only with the write deferred to Step 6, so without a union of the new sites two parallel agents could land in one file; the rule merges same-round clusters that come to share a file and re-derives the two-file limb | — |
| E44 | Minted task 22 to gate the README capabilities sample version literal | 2026-09-18 | | Cheap (one assertion), inside a file the run touched, and covered by the capabilities suite the run already runs - all three same-run factors hold | — |
| E52 | items clusters dropped_deps entries are {id, unselected: [{id, status}], unknown: [ids]} instead of {id, missing} (review R16, user-confirmed shape) | 2026-09-18 | | missing flattened a terminal dependency (safe to drop), an open item outside --ids (an ordering hazard) and an unknown id into one list whose name reads as absent from the ledger; the split with status lets the apply orchestrator audit each dropped edge without an items get per id | — |
| E53 | Enumeration.files is Vec<String> rather than Vec<PathBuf> (review R13) | 2026-09-18 | | the values are git's /-spelled repo-relative strings and every consumer converted back to a string; PathBuf advertised the root.join operation the module itself warns against under a verbatim root | — |
| E54 | items sweep consumes the engine's scanned/skipped sets and shared scan_file/line_of instead of re-enumerating and re-scanning; run split into compile + run_compiled (review R2) | 2026-09-18 | | the duplicated predicate decided gone-vs-unverified, so any drift between the two copies silently misclassified anchors; SweepReport.scanned and .skipped plus sweep::scan_file give one source of truth and one injection seam | E22 |
| E55 | items_sweep returns a typed SweepOutcome and update_plan takes &SweepOutcome; outcome_json emits the wire shape (review R12) | 2026-09-18 | | the write path re-parsed the wire JSON with missing-key-means-empty semantics, so a renamed envelope key disabled the refusal gate with no compile error; the typed shape mirrors SweepReport + report_json | — |
| E56 | git_available, git and write test helpers live once in test_support.rs (review R14) | 2026-09-18 | | four copies of git_available and two each of git and write had accumulated; test_support.rs is the crate's home for helpers shared across unit-test modules | — |
| E57 | a file:symbol anchor covers every hit in its indentation span, not only the symbol's line (review R6, user-confirmed rule) | 2026-09-18 | | the first-line rule made --update write a drift-fragile file:line twin for every hit inside an anchored function body, defeating the purpose of symbol anchors; the span ends at the next non-blank line at <= indentation that is not a bare closing bracket, degrading to first-line-only for Markdown and TOML | — |
| E58 | unverified entries carry {anchor, reason}; --update retains excluded anchors in listed order and refuses only for truncation or a non-excluded unverified anchor (review R8, user-confirmed) | 2026-09-18 | | an anchor under a default-excluded path was permanently unverified and blocked --update forever with a count-only message; reasons (unparseable, outside-repo, truncated, missing, skipped, excluded) make the refusal actionable and let excluded anchors survive a rewrite | — |
| E59 | items sweep --update rewrites only open items; terminal items are reported but never rewritten, even under --ids (review R5) | 2026-09-18 | | a fixed or wontfix item whose patterns now hit nothing was having its instances unset, erasing the record of what was fixed; items clusters already scopes its bare selection to open, and items update remains the way to edit a terminal item's instances | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-09-18 | 59 entries: status-transition × 2, task-completion × 22, deviation × 17, verification × 15, checkpoint × 3 | 037f595, 2b43620, 434bc3c, 44fcbc4, 56a1a01, 6c44c56, 81b9bb1, 987a414, 9ccb182, b16ece2, baf9f37, c4a2322, c95ff36, d465972, d551559, f84446e |

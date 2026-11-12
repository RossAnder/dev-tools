Status: done — the poller serves document requests (authored file, cited source, commit, finding) read in source/doc.rs and answered with Event::Doc; the runtime holds the latest answer and drops stale ones.

## What changed

- `glimpse/src/source/doc.rs` (was a one-line stub): `DocRequest` (Authored, Source, Commit, Finding), `Fetched { title, text, notices, line, markdown }`, the notice constants `read uncommitted`, `commit not recorded` and `changed since`, and `fetch(root, request) -> Result<Fetched, String>`.
  - Authored goes through `tomlctl::authored_read`. It is marked markdown when the path ends `.md`.
  - Finding goes through `tomlctl::review_finding`, shown as `key: value` lines in the projection's order.
  - Commit runs `git show --no-color --stat --format=fuller <sha> --`.
  - Source runs `git show <read_at>:<path>` when `read_at` is set and the file was not dirty. It adds `changed since` when the working-tree text differs or the file is absent. Otherwise it reads the working tree with the right notice or notices.
  - Git runs from the repo root with separate arguments. A sha or `read_at` must be 7 to 64 hex digits. A path must be non-empty, contain no backslash and consist only of `Component::Normal` parts. A working-tree read also canonicalises the path and requires it to stay under the canonical root.
  - Files over 2 MiB and non-UTF-8 files are refused. Text is LF-normalised, so a CRLF checkout is not reported as `changed since`.
  - Tests use a temp git repo and cover: a clean source at an older commit, a source deleted since its commit, a dirty source, no `read_at`, refusals (path outside the root, absolute path, backslash path, empty path, non-hex or short sha, option-like sha, oversized file, non-UTF-8 file), a commit's stat, a finding's fields, and an authored read.
- `glimpse/src/source.rs`: `Control::Doc(Option<DocRequest>)`, `Event::Doc { request, document }`, `Poller.doc` (the pending request), `Poller::serve_doc`, and `Source::set_doc`.
  - A request is held as pending. The run loop's wait drops to zero while one is pending, so it is served only once the control queue is empty. A newer request queued behind it replaces it unread, and `None` withdraws it. Neither `Tail` nor `Doc` forces a tick.
  - New test: a request queued behind another replaces it, and a withdrawn request is not served.
- `glimpse/src/runtime.rs`: `Host::set_doc` (TerminalHost forwards it to `Source::set_doc`), `Screen.doc: Document { wanted, shown }`, `want_doc` (sends only on change and clears the old answer), and an `Event::Doc` arm that drops an answer whose request is not the wanted one. FakeHost records the doc requests. New test: an answer for a document no longer wanted is dropped, the current one is held, and withdrawing clears it.

## Deviations

- `Fetched`'s fields are `pub(crate)`. The means gives them without visibility, but the next task's view module must read them.
- `--no-color` and a trailing `--` are added to the `git show` commit command, as defensive extras. Output is unchanged when git writes to a pipe.
- Nothing calls `want_doc`, `Host::set_doc`, `Source::set_doc` or `Document.shown` outside tests until the next task wires the details panel. They carry `#[cfg_attr(not(test), allow(dead_code))]`, the crate's existing idiom (as on `Event::Appearance`), so the non-test build stays warning-free. The next task can drop those attributes.
- The working tree's text is LF-normalised both for display and for the `changed since` comparison. Otherwise every CRLF checkout on Windows would read as changed against git's LF blob.

## Checks

- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`: clean for this task's files. Its 5 dead-code warnings name only `authored.rs`, `model.rs` and `model/plan.rs`, which other tasks in this milestone have in flight.
- `rustfmt --edition 2024 --check` on doc.rs, source.rs and runtime.rs: clean after applying rustfmt.
- Accept check "the request type exists and the poller serves it" (grep): pass.
- Accept check "the document reads carry their own tests" (grep): pass.
- Accept check `cargo test --manifest-path glimpse/Cargo.toml source::doc`: not run, since it is the gate's. The tests compile under clippy `--all-targets`. They need `git` on PATH.

TANGENTIAL: none

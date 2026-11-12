Status: done — tomlctl::authored_list and tomlctl::authored_read list and read a flow's brief, design files, notes, run notes and settle notes, held inside the flow's directory

## What changed

- `tomlctl/src/plan/authored.rs`: `list(root, slug)` and `read(root, slug, path)`. Both validate the slug, then build the anchor as `root.canonicalize()/docs/flows/<slug>`, the same shape as `store::authored_anchor` but from the `root` argument, never the process root. A path is classified by place and name (`brief.md`, `design/*.md`, `notes/*.md` split into note, challenge, cold-read and backlog, `notes/run/task-<n>.md`, `notes/settle/*.md`). Anything else is left out, and so is any path holding `\`, `:`, an empty segment, `.` or `..`. Each candidate is canonicalised and kept only when it is a file inside the anchor. Headline and state come from the first line by kind. Run notes go through `run_note::parse`, and a note with no Status line falls back to its first line with state `""`. Sources come from `note_sources::parse_note_sources`, whose existence predicate is `root.join(p).exists()`, and only note, challenge and cold-read files carry them. The text is read lossily and has `\r\n` turned into `\n`. A missing directory lists nothing, and the result is sorted by `path`. `read` refuses a path that does not classify, or that is absent or resolves outside the anchor. Two unit tests are inline (classification, and the fallback for a run note with no Status line).
- `tomlctl/src/lib.rs`: two new `pub fn`s, `authored_list` and `authored_read`, each calling `io::silence_advisories()` and then delegating. No existing facade function changed.
- `tomlctl/tests/library_facade.rs`: five cases over a sandbox root:
  - every kind listed and sorted, with id and headline, including a settle note;
  - a run note's `task` and `state`;
  - sources from an inline reference, with a nonexistent path dropped, and from a `toml sources` block;
  - `[]` sources for the kinds that carry none;
  - a missing directory lists nothing;
  - a bad slug is refused by both functions;
  - a CRLF brief reads back with LF only;
  - unlisted, `..`, backslash and missing paths are refused on read;
  - a symlinked note pointing outside the flow is left out and refused. This case returns early where the platform will not create a symlink.

## Deviations

- `read` does not call `list`. It classifies the requested path with the same rules and runs the same canonical check. The result is the same set of accepted paths without parsing every file on each read. I also considered matching against the full listing, which is simpler to reason about but parses every note on every read.
- The headline fallbacks are my own choice. A note whose first line lacks `Verdict:` shows that line as it is, and so does a brief or design file whose first line lacks `# `. The design table does not say what to do in either case.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check` on `authored.rs` and `library_facade.rs`: clean after formatting. `lib.rs` was not checked because it has `mod` children.
- Accept check "the library exposes the listing and the read" (grep): pass.
- Accept check "the facade tests cover the authored listing" (grep): pass.
- The `binary(library_facade)` nextest check is the gate's and was not run.

## Security

This is a path boundary. Two layers stop a caller-supplied `path` or a planted symlink from reading outside `docs/flows/<slug>`. First, the lexical whitelist in `classify` rejects `..`, `.`, empty segments, `\` and `:`. Second, the canonicalised file must start with the canonical root joined with `docs/flows/<slug>`, which catches a symlink at the file or at any directory under the root. The slug goes through `flow::validate_slug` before any path is built. Nothing is written.

TANGENTIAL: none

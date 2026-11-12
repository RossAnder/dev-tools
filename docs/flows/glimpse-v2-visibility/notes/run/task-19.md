Status: done — the poller feeds the viewed flow's authored-files listing, re-statted every scan and woken by a docs/flows watch

## What changed

- `glimpse/src/watch.rs`: `Wake::Authored { slug }`; `Roots` gains the `docs/flows` path, derived from the canonical `.claude` so it is one path whether or not it exists yet. `place` classifies a path under it: a `.md` file inside `docs/flows/<slug>/` wakes that slug, an extensionless entry (a directory) arriving or departing wakes it, any other file wakes nothing, and `docs/flows` itself departing is a `Rewatch`, as for the flows root. New `add_tree` adds a recursive watch on a directory unless it is missing. Classification tests and a live test of a note written under a tree added after start.
- `glimpse/src/source/mode.rs`: `WatchScope::Authored`; `Wakes.authored` (counted by `is_empty` and `woken`); `Mode::Watching` carries `authored: bool`. `Mode::probe_authored` adds the `docs/flows` watch once it exists, at most once; `Mode::start` probes it once, and `watches_authored` reports it. Tests for a `docs/flows` created after start and for the authored wake's scope.
- `glimpse/src/source/scan.rs`: `FeedStat` (`File` or `Tree`) and `feed_stat`, the authored detector: it walks `docs/flows/<slug>/` on each scan and stats every `.md` file without descending linked directories. A slug that `tomlctl::validate_slug` refuses walks nothing. `authored_dir`; `Fetcher::fetch_authored`, which `InProcessFetcher` serves through `tomlctl::authored_list`; fake fetcher support; a stat test.
- `glimpse/src/source.rs`: `Feed::Authored { slug }` with scope `WatchScope::Authored`, woken by `Wakes.authored`; `Event::Authored { slug, listing: Result<(AuthoredList, u64), String> }`. `FeedState.stat` is now a `FeedStat`, so `poll_feeds` reads the listing only when a stat moved, a file came or went, or a wake evicted the feed. A failure is sent once and retried, as for the other feeds. The revision is a hash of the listing document, so a re-read that lists the same thing is not resent. `run` probes `docs/flows` before every tick and drops `WatchScope::Authored` from the changed set while no watch covers it, so such a change counts no miss. Poller tests: a new note after a wake, an in-place edit seen by re-stat with no wake, an unrelated file reads nothing, a missing `docs/flows` costs no error and is picked up once created, and a failed read is reported once.
- `glimpse/src/runtime.rs`: `feeds_for` subscribes `Feed::Authored` for the slug the poller follows, so it resubscribes when the flow changes. `Event::Authored` reaches `App::apply_authored`, and a read for an unsubscribed feed is dropped. A listing that arrives before its flow's snapshot is held in `Pointed` and applied after `apply_snapshot`, which clears the listing on a flow change. An error goes to `notify_error`. The test helpers are updated, and there are two new tests.

## Deviations

- The required means replaced the suggested `Wake::NewScope`-style variant. `docs/flows` is probed before each tick and its arrival goes through no wake, as the required means says.
- Revision: `App::apply_authored` takes a `u64`, and `tomlctl::authored_list` carries no revision. I used an in-process `DefaultHasher` hash of the listing JSON, which is stable within one run, and that is all it needs. The alternative was a read counter: it is simpler, but it would resend an unchanged listing after every wake.
- Directory events under `docs/flows/<slug>/` wake their slug on arrival or departure. The detail says "ignore non-`.md` paths", but a moved directory reports only its own path. Without this, a renamed `notes/` would be found only by the re-stat and would count as a miss against a healthy watch. An extensionless directory change (dir-entry modify) still wakes nothing.
- Runtime holding of an early listing: if the listing arrived before its flow's snapshot, `apply_snapshot` would clear it, and the poller would not resend an unchanged revision. Holding the listing in `Pointed` avoids that. The other option was to drop it and force a re-read, which costs a second read and needs a new control message.

## Handover

None.

## Checks

- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`: clean in this task's files. Warnings remain only for unused items in `authored.rs`, `model.rs` and `model/plan.rs`, which other tasks own.
- `rustfmt --edition 2024 --check` on `source.rs` (and its children), `watch.rs` and `runtime.rs`: clean after applying it. `source/doc.rs` was untouched.
- Narrow tests: `cargo test --bin glimpse authored` ran 13 tests and all passed. A second run with the filters for the watch, `docs/flows`, switching-flow, inputs and auto-flow tests ran 10 and all passed.
- Accept check 1 (feed and scope exist): pass. Accept check 3 (`fn [a-z_]*authored` in source.rs/scan.rs): pass. Contract C2's check: pass.
- Accept check 2 (`cargo test ... source::`) is the gate's to run; I did not run it.

TANGENTIAL: debt | glimpse/src/source/scan.rs | `markdown_stats` walks the whole `docs/flows/<slug>/` tree every scan, including subdirectories tomlctl does not list | cheap at about 70 files, but it grows with any non-note subtree a flow adds

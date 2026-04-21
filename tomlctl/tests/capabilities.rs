//! Capability-surface integration tests split out of the monolithic
//! `integration.rs` by R23. Covers everything the T7 capabilities JSON
//! advertises — `--count-distinct` (T1), `--raw` (T2), `--lines` (T3),
//! `--error-format` (T8), `--strict-read` (T9), the `capabilities`
//! subcommand (T7) itself, the read-/write-subcommand `--help` snapshots
//! that pin the global-flag visibility surface (R44 + R74), and the T11
//! backfill → T1 count-distinct end-to-end contract (R45). Every test
//! body is byte-identical to its pre-split form; helpers live in
//! `tests/common/mod.rs`.

use assert_cmd::Command;
use std::fs;

mod common;
use common::{parse_json_error_envelope, run_list_query, run_list_query_with, seed_ledger, QUERY_FIXTURE};

/// R74: read-only subcommands (`parse`, `get`, `validate`, `items list`,
/// `items get`, `items find-duplicates`, `items orphans`, `items next-id`)
/// must NOT expose the write-side integrity flags (`--allow-outside`,
/// `--no-write-integrity`, `--strict-integrity`). They still accept
/// `--verify-integrity` because that's the only read-side integrity
/// concept. A test-per-flag per-subcommand would be noisy — inspect the
/// rendered `--help` text and assert the write-side flags don't appear.
#[test]
fn read_only_subcommands_hide_write_integrity_flags_in_help() {
    let read_subs: &[&[&str]] = &[
        &["parse", "--help"],
        &["get", "--help"],
        &["validate", "--help"],
        &["items", "list", "--help"],
        &["items", "get", "--help"],
        &["items", "find-duplicates", "--help"],
        &["items", "orphans", "--help"],
        &["items", "next-id", "--help"],
    ];
    for path in read_subs {
        let mut cmd = Command::cargo_bin("tomlctl").unwrap();
        for a in *path {
            cmd.arg(a);
        }
        let assert = cmd.write_stdin("").assert().success();
        let stdout =
            String::from_utf8_lossy(&assert.get_output().stdout).to_string();
        // --verify-integrity is allowed on read paths; present is fine.
        for banned in ["--allow-outside", "--no-write-integrity", "--strict-integrity"] {
            assert!(
                !stdout.contains(banned),
                "read-only sub `{}` must NOT list `{}` in --help; got:\n{}",
                path.join(" "),
                banned,
                stdout
            );
        }
    }
}

/// R74 (complement): write subcommands MUST continue to list every integrity
/// flag in `--help`. Pins the structural guarantee that the split didn't
/// accidentally strip a flag from a writer.
#[test]
fn write_subcommands_expose_all_integrity_flags_in_help() {
    let write_subs: &[&[&str]] = &[
        &["set", "--help"],
        &["set-json", "--help"],
        &["array-append", "--help"],
        &["items", "add", "--help"],
        &["items", "update", "--help"],
        &["items", "remove", "--help"],
        &["items", "apply", "--help"],
        &["items", "add-many", "--help"],
    ];
    for path in write_subs {
        let mut cmd = Command::cargo_bin("tomlctl").unwrap();
        for a in *path {
            cmd.arg(a);
        }
        let assert = cmd.write_stdin("").assert().success();
        let stdout =
            String::from_utf8_lossy(&assert.get_output().stdout).to_string();
        for required in [
            "--allow-outside",
            "--no-write-integrity",
            "--verify-integrity",
            "--strict-integrity",
        ] {
            assert!(
                stdout.contains(required),
                "write sub `{}` must list `{}` in --help; got:\n{}",
                path.join(" "),
                required,
                stdout
            );
        }
    }
}

/// R76: `--count`, `--count-by`, `--group-by`, `--pluck` are declared as a
/// mutually exclusive clap ArgGroup on `items list`. Two of them on the
/// same command must fail at parse time with clap's "cannot be used with"
/// error — not silently collapse to one shape via the `build_query`
/// priority ladder. `--ndjson` is orthogonal (a separate output encoding,
/// not a shape) and is NOT in the group.
#[test]
fn items_list_shape_flags_are_mutually_exclusive_at_parse_time() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--count")
        .arg("--count-by")
        .arg("status")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("cannot be used with")
            || stderr.contains("argument cannot be used"),
        "expected clap mutex error, got stderr:\n{stderr}"
    );
    // --ndjson + --count-by must still be parse-accepted (they're orthogonal;
    // the runtime may still reject it via validate_query, but it MUST NOT be
    // rejected by the ArgGroup). Only assert that stderr does NOT carry the
    // ArgGroup mutex phrase for this pair.
    let out2 = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--count-by")
        .arg("status")
        .arg("--ndjson")
        .write_stdin("")
        .assert();
    // Accept either success OR a validate-layer runtime error — just not
    // the clap ArgGroup "cannot be used" phrase, which would mean --ndjson
    // leaked into the shape group by mistake.
    let stderr2 = String::from_utf8_lossy(&out2.get_output().stderr).to_string();
    assert!(
        !stderr2.contains("cannot be used with"),
        "--ndjson must stay OUTSIDE the shape ArgGroup (R82 + R76); got stderr:\n{stderr2}"
    );
}

// ---------------------------------------------------------------------------
// Task 1 (plan `docs/plans/tomlctl-capability-gaps.md`): `items list` grows
// `--count-distinct <FIELD>`, a scalar-cardinality aggregate that replaces
// the 4-stage `--pluck X | jq -r '.[]' | sort -u | wc -l` pipe chain agents
// were spelling out. Output: `{"count_distinct":N,"field":"<name>"}`.
// Null/missing field values are excluded (`--pluck` semantics). The flag
// joins the existing `shape` ArgGroup, so pairwise-mutex with every other
// aggregation shape is enforced at clap parse time.
// ---------------------------------------------------------------------------

/// T1: end-to-end happy path. Fixture has 3 distinct categories across 6
/// rows — output shape must be `{count_distinct:3, field:"category"}`.
#[test]
fn items_list_count_distinct_emits_expected_object() {
    let stdout = run_list_query(&["--count-distinct", "category"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    assert_eq!(
        v.get("count_distinct").and_then(|n| n.as_u64()),
        Some(4),
        "QUERY_FIXTURE has 4 distinct categories (style, bug, perf, security); got stdout:\n{stdout}"
    );
    assert_eq!(
        v.get("field").and_then(|s| s.as_str()),
        Some("category"),
        "`field` must echo the flag arg back; got stdout:\n{stdout}"
    );
}

/// T1: `--count-distinct` composes with `--where` — the distinct count is
/// over the FILTERED set. Same contract as Count / CountBy.
#[test]
fn items_list_count_distinct_composes_with_where() {
    // QUERY_FIXTURE open items: R1 (style), R2 (bug), R4 (perf), R6
    // (security) → 4 distinct categories.
    let stdout = run_list_query(&["--where", "status=open", "--count-distinct", "category"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    assert_eq!(
        v.get("count_distinct").and_then(|n| n.as_u64()),
        Some(4),
        "open items span 4 distinct categories; got stdout:\n{stdout}"
    );
}

/// T1 / Risk #2: `--count-distinct` and `--pluck` both in the same call
/// must error at clap parse time (via the `shape` ArgGroup), NOT
/// silently collapse via the build_query priority ladder.
#[test]
fn count_distinct_and_pluck_are_mutex_at_parse_time() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--pluck")
        .arg("id")
        .arg("--count-distinct")
        .arg("category")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("cannot be used with")
            || stderr.contains("argument cannot be used"),
        "expected clap ArgGroup mutex error on --pluck + --count-distinct; got stderr:\n{stderr}"
    );
}

/// T1: `--count-distinct` + `--count` also errors at clap (same
/// ArgGroup). Pins that the ArgGroup was extended, not a new disjoint
/// group created.
#[test]
fn count_distinct_with_count_errors_at_clap() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--count")
        .arg("--count-distinct")
        .arg("category")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("cannot be used with")
            || stderr.contains("argument cannot be used"),
        "expected clap ArgGroup mutex error on --count + --count-distinct; got stderr:\n{stderr}"
    );
}

/// T1: `--count-distinct` + `--select` errors via `validate_query`, which
/// T8 tagged `kind=validation`. Assert both the human-readable mutex
/// wording and (with `--error-format json`) the structured kind tag.
#[test]
fn count_distinct_with_select_errors_via_validate_query() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);

    // Text mode: anyhow chain contains both flag names.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--count-distinct")
        .arg("category")
        .arg("--select")
        .arg("id")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("--select") && stderr.contains("--count-distinct"),
        "text-mode error must name both flags; got stderr:\n{stderr}"
    );

    // JSON mode: `kind=validation` tag surfaces.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("--error-format")
        .arg("json")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--count-distinct")
        .arg("category")
        .arg("--select")
        .arg("id")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let envelope: serde_json::Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("json-mode stderr must parse: {e}; stderr:\n{stderr}"));
    assert_eq!(
        envelope
            .get("error")
            .and_then(|e| e.get("kind"))
            .and_then(|s| s.as_str()),
        Some("validation"),
        "expected kind=validation; got stderr:\n{stderr}"
    );
}

// ---------------------------------------------------------------------------
// Task 3 (plan `docs/plans/tomlctl-capability-gaps.md`): `--lines` and
// `--pluck` + `--ndjson` composition. `--lines` is a discoverable spelling
// of `--ndjson` for the Pluck case; both flags enable one-value-per-line
// streaming. Aggregation shapes silently treat the bit as a no-op.
// ---------------------------------------------------------------------------

/// 4-row fixture whose items each carry a `x` string field. Kept as a
/// module-local const to avoid dragging the generic `QUERY_FIXTURE` into
/// tests that only need a tiny pluck surface.
const PLUCK_FIXTURE: &str = r#"schema_version = 1

[[items]]
id = "R1"
x = "v1"

[[items]]
id = "R2"
x = "v2"

[[items]]
id = "R3"
x = "v3"

[[items]]
id = "R4"
x = "v4"
"#;

/// T3-1: `--pluck x --lines` emits one quoted JSON string per line. Asserts
/// the exact byte sequence so a future refactor that e.g. emits bare
/// strings (T2's `--raw` territory) trips this test rather than silently
/// changing the contract.
#[test]
fn lines_with_pluck_emits_one_json_value_per_line() {
    let stdout = run_list_query_with(PLUCK_FIXTURE, &["--pluck", "x", "--lines"]);
    assert_eq!(stdout, "\"v1\"\n\"v2\"\n\"v3\"\n\"v4\"\n");
}

/// T3-2: `--pluck x --ndjson` is byte-identical to `--pluck x --lines`.
/// The two spellings are aliases at the semantic level — this test pins
/// the identity so future work can't accidentally diverge them.
#[test]
fn ndjson_with_pluck_is_byte_identical_to_lines_with_pluck() {
    let lines_out = run_list_query_with(PLUCK_FIXTURE, &["--pluck", "x", "--lines"]);
    let ndjson_out = run_list_query_with(PLUCK_FIXTURE, &["--pluck", "x", "--ndjson"]);
    assert_eq!(
        lines_out, ndjson_out,
        "--lines and --ndjson must be byte-identical on --pluck"
    );
}

/// T3-3: `--lines` composes with `--distinct` and `--sort-by`. The slow-path
/// branch of `run_streaming` handles these; this test pins that sort/distinct
/// still apply in the streaming emit order.
#[test]
fn lines_with_pluck_distinct_and_sort() {
    let fixture = r#"schema_version = 1

[[items]]
id = "R1"
x = "gamma"

[[items]]
id = "R2"
x = "alpha"

[[items]]
id = "R3"
x = "alpha"

[[items]]
id = "R4"
x = "beta"
"#;
    let stdout = run_list_query_with(
        fixture,
        &["--pluck", "x", "--lines", "--distinct", "--sort-by", "x:asc"],
    );
    assert_eq!(stdout, "\"alpha\"\n\"beta\"\n\"gamma\"\n");
}

/// T3-4: `--lines` composes with `--limit` — exactly N lines in the output.
/// Catches a regression where the streaming slow path fails to honour
/// `apply_window`.
#[test]
fn lines_with_pluck_and_limit() {
    let stdout = run_list_query_with(
        PLUCK_FIXTURE,
        &["--pluck", "x", "--lines", "--limit", "2"],
    );
    let line_count = stdout.lines().count();
    assert_eq!(line_count, 2, "expected 2 lines with --limit 2; got:\n{stdout}");
    assert_eq!(stdout, "\"v1\"\n\"v2\"\n");
}

/// T3-5: `--lines` shows up in `items list --help` as a discrete entry.
/// Clap aliases don't render in help, so this test is the structural guard
/// against someone "simplifying" the flag into `alias = "lines"`.
#[test]
fn lines_flag_listed_in_items_list_help() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("items")
        .arg("list")
        .arg("--help")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains("--lines"),
        "items list --help must list --lines as a discrete flag; got:\n{stdout}"
    );
    // Both flags should be visible — the point of T3 is that --ndjson and
    // --lines coexist, not that one replaces the other.
    assert!(
        stdout.contains("--ndjson"),
        "items list --help must still list --ndjson alongside --lines; got:\n{stdout}"
    );
}

/// T3-6: `--lines` on a non-Pluck/non-Array shape is a silent no-op. For
/// Count the output is a single `{"count": N}` object regardless — per-line
/// decomposition has no meaning. Agents can blanket-add `--lines` to
/// scripts without branching on shape.
#[test]
fn lines_on_count_shape_is_noop_single_object() {
    let stdout = run_list_query_with(PLUCK_FIXTURE, &["--count", "--lines"]);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must parse as a single JSON value: {e}; stdout:\n{stdout}"));
    assert_eq!(
        v.get("count").and_then(|n| n.as_u64()),
        Some(4),
        "expected {{count: 4}}; got:\n{stdout}"
    );
    // Structural guard: the whole stdout is a single parseable JSON object,
    // not a sequence of per-line JSON values. The pretty-print formatter
    // splits the object across multiple display lines — that's fine, what
    // matters is that there's exactly one top-level JSON value.
    assert!(
        v.is_object(),
        "Count + --lines must emit a single top-level JSON object; got:\n{stdout}"
    );
    // Byte-identical parity vs the same query without `--lines` — proves
    // --lines is a true no-op on Count.
    let stdout_no_lines = run_list_query_with(PLUCK_FIXTURE, &["--count"]);
    assert_eq!(
        stdout, stdout_no_lines,
        "--lines on --count must be a byte-identical no-op"
    );
}

/// T3-7: null/missing plucked values are dropped in streaming — same
/// contract as `apply_pluck` in the non-streaming path. Pins the parity
/// constraint that motivated mirroring the `None | Some(JsonValue::Null)`
/// match in `run_streaming`.
#[test]
fn lines_with_pluck_drops_null_and_missing_fields() {
    let fixture = r#"schema_version = 1

[[items]]
id = "R1"
x = "v1"

[[items]]
id = "R2"

[[items]]
id = "R3"
x = "v3"
"#;
    // R2 is missing `x` entirely — it must not appear as `null\n` or as an
    // empty line in the output.
    let stdout = run_list_query_with(fixture, &["--pluck", "x", "--lines"]);
    assert_eq!(stdout, "\"v1\"\n\"v3\"\n");
    // Non-streaming path must drop the same items (byte-set parity).
    let stdout_array = run_list_query_with(fixture, &["--pluck", "x"]);
    let arr: serde_json::Value = serde_json::from_str(&stdout_array)
        .unwrap_or_else(|e| panic!("--pluck x (no lines) must be JSON: {e}; stdout:\n{stdout_array}"));
    assert_eq!(arr, serde_json::json!(["v1", "v3"]));
}

// ---------------------------------------------------------------------------
// Task 8 (plan `docs/plans/tomlctl-capability-gaps.md`): `--error-format
// {text,json}` global flag + closed `ErrorKind` taxonomy. Tagged call sites:
//   - io.rs `read_toml` / `read_toml_str` missing-file      -> kind=not_found
//   - io.rs `read_toml` / `read_doc_borrowed` TOML parse    -> kind=parse
//   - integrity.rs `verify_integrity` sidecar failure       -> kind=integrity
//   - query.rs `validate_query` mutex violations            -> kind=validation
//   - items.rs `items_next_id` prefix-shape validation      -> kind=validation
// Every other `bail!` / `anyhow!` falls through to kind=other. Exit code
// stays 1 regardless of format. Text-mode output is byte-identical to the
// pre-T8 `eprintln!("tomlctl: {:#}", err)` stream.
// ---------------------------------------------------------------------------

/// T8 Test 1: missing-file path -> `kind=not_found`. `items get` on a
/// nonexistent file is the cleanest trigger — it goes straight through
/// `read_toml`'s NotFound arm with the path known, so the envelope also
/// carries a non-null `file` field.
#[test]
fn error_format_json_missing_file_tagged_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let missing = claude.join("nope.toml");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("get")
        .arg(&missing)
        .arg("R1")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], serde_json::json!("not_found"));
    let message = err["message"].as_str().unwrap();
    assert!(
        message.contains("No such file") || message.contains("not found"),
        "expected missing-file prose in message, got: {message}"
    );
    let file = err["file"].as_str().expect("file must be populated on not_found");
    assert!(
        file.contains("nope.toml"),
        "file field must carry the target path, got: {file}"
    );
}

/// T8 Test 2: sidecar hash mismatch -> `kind=integrity`. Write a valid TOML
/// with a deliberately-wrong sidecar; `--verify-integrity` triggers
/// `integrity.rs::verify_integrity` which tags the mismatch.
#[test]
fn error_format_json_sidecar_mismatch_tagged_integrity() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let file = claude.join("data.toml");
    fs::write(&file, "key = \"value\"\n").unwrap();
    // A 64-hex-char digest that will NEVER match the real hash of the file.
    let mut sidecar = file.clone().into_os_string();
    sidecar.push(".sha256");
    fs::write(
        &sidecar,
        "deadbeef00000000000000000000000000000000000000000000000000000000  data.toml\n",
    )
    .unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("get")
        .arg(&file)
        .arg("key")
        .arg("--verify-integrity")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], serde_json::json!("integrity"));
    let message = err["message"].as_str().unwrap();
    assert!(
        message.contains("integrity check failed")
            && message.contains("expected")
            && message.contains("actual"),
        "expected dual-digest message, got: {message}"
    );
    let file_field = err["file"].as_str().unwrap();
    assert!(
        file_field.contains("data.toml"),
        "file must name the verified path, got: {file_field}"
    );
}

/// T8 Test 3: TOML parse error -> `kind=parse`. Malformed TOML, `parse`
/// subcommand. Exercises the borrowed fast-path (`read_doc_borrowed`) since
/// `--verify-integrity` is absent. Owned path (`read_toml`) is covered
/// transitively by any `items list` / `get` / `items get` on the same bad
/// fixture; the parse subcommand is the cleanest fixture here.
#[test]
fn error_format_json_bad_toml_tagged_parse() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let file = claude.join("bad.toml");
    // A clearly invalid TOML: bare `=` with no RHS.
    fs::write(&file, "malformed = =\n").unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("parse")
        .arg(&file)
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], serde_json::json!("parse"));
    let message = err["message"].as_str().unwrap();
    assert!(
        message.contains("parse")
            && (message.contains("borrowed TOML")
                || message.contains("parsing")),
        "expected TOML parse prose, got: {message}"
    );
}

/// T8 Test 4: query mutex violation -> `kind=validation`. `items list
/// --select x --exclude y` is rejected inside `validate_query`'s first
/// branch. Uses an existing (empty-items) ledger so the file read succeeds
/// and the error genuinely comes from `validate_query`, not `read_toml`.
#[test]
fn error_format_json_query_mutex_tagged_validation() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let file = claude.join("empty.toml");
    fs::write(&file, "schema_version = 1\n").unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("list")
        .arg(&file)
        .arg("--select")
        .arg("a")
        .arg("--exclude")
        .arg("b")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], serde_json::json!("validation"));
    let message = err["message"].as_str().unwrap();
    assert!(
        message.contains("--select and --exclude are mutually exclusive"),
        "expected validate_query mutex prose, got: {message}"
    );
    assert!(err["file"].is_null(), "query validation has no file hint");
}

/// T8 Test 5: `items_next_id` prefix validation -> `kind=validation`. Pass
/// `--prefix ""` against an EXISTING (empty-items) ledger so control reaches
/// `items_next_id`'s empty-prefix check (the cli.rs missing-file fast path
/// has its own untagged bail, which isn't the plan's tag site).
#[test]
fn error_format_json_next_id_empty_prefix_tagged_validation() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let file = claude.join("ledger.toml");
    fs::write(&file, "schema_version = 1\n").unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("next-id")
        .arg(&file)
        .arg("--prefix")
        .arg("")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], serde_json::json!("validation"));
    let message = err["message"].as_str().unwrap();
    assert!(
        message.contains("prefix must not be empty"),
        "expected prefix-empty validation message, got: {message}"
    );
}

/// T8 Test 6: untagged error -> `kind=other`. `items get <file> <missing-id>`
/// errors inside `items_get_from` (not on the plan's closed list), so it
/// should fall through to the generic `other` bucket. Confirms the default
/// fallback works for every un-annotated bail in the codebase.
#[test]
fn error_format_json_untagged_fallback_kind_other() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let file = claude.join("ledger.toml");
    fs::write(
        &file,
        r#"schema_version = 1

[[items]]
id = "R1"
summary = "present"
"#,
    )
    .unwrap();

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("get")
        .arg(&file)
        .arg("R999")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(
        err["kind"],
        serde_json::json!("other"),
        "untagged errors must fall through to kind=other"
    );
    let message = err["message"].as_str().unwrap();
    assert!(
        message.contains("no item with id = R999"),
        "expected item-not-found prose in other-kind message, got: {message}"
    );
    assert!(
        err["file"].is_null(),
        "other-kind errors have no file hint (no TaggedError in chain)"
    );
}

/// T8 Test 7: text-mode regression — when `--error-format` is absent the
/// stderr stream is byte-identical to the pre-T8 `tomlctl: {:#}` line. Spot
/// checks three of the tagged kinds (not_found, validation-query,
/// validation-next-id) to pin no-prefix / no-bracketed-annotation rendering.
#[test]
fn error_format_text_mode_byte_identical_across_tag_kinds() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let empty_ledger = claude.join("empty.toml");
    fs::write(&empty_ledger, "schema_version = 1\n").unwrap();
    let missing = claude.join("missing.toml");

    // Spot 1: not_found — missing-file path via items get.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("get")
        .arg(&missing)
        .arg("R1")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.starts_with("tomlctl: reading "),
        "pre-T8 prefix must be unchanged, got: {stderr:?}"
    );
    assert!(
        !stderr.contains("[not_found]") && !stderr.contains("{\"error\""),
        "text mode must NOT leak tag prefix or JSON envelope, got: {stderr:?}"
    );

    // Spot 2: validation — query mutex.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("list")
        .arg(&empty_ledger)
        .arg("--select")
        .arg("a")
        .arg("--exclude")
        .arg("b")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert_eq!(
        stderr.trim_end(),
        "tomlctl: --select and --exclude are mutually exclusive",
        "text-mode validation output must be byte-identical"
    );

    // Spot 3: validation — next-id empty prefix on existing file.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("next-id")
        .arg(&empty_ledger)
        .arg("--prefix")
        .arg("")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert_eq!(
        stderr.trim_end(),
        "tomlctl: prefix must not be empty — use a letter like R, O, or A",
        "text-mode next-id validation output must be byte-identical"
    );
}

/// T8: `--error-format json` is a global flag — caller can place it BEFORE
/// or AFTER the subcommand name with identical behaviour. Pin both positions
/// against a missing-file trigger so the `global = true` attribute doesn't
/// silently regress.
#[test]
fn error_format_json_flag_position_is_global() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let missing = claude.join("missing.toml");

    // Flag BEFORE subcommand.
    let out_before = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("--error-format")
        .arg("json")
        .arg("items")
        .arg("get")
        .arg(&missing)
        .arg("R1")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr_before =
        String::from_utf8_lossy(&out_before.get_output().stderr).to_string();
    let env_before = parse_json_error_envelope(&stderr_before);
    assert_eq!(env_before["kind"], serde_json::json!("not_found"));

    // Flag AFTER subcommand (and after the file/id args).
    let out_after = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("get")
        .arg(&missing)
        .arg("R1")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr_after = String::from_utf8_lossy(&out_after.get_output().stderr).to_string();
    let env_after = parse_json_error_envelope(&stderr_after);
    assert_eq!(env_after["kind"], serde_json::json!("not_found"));

    // Envelopes match byte-for-byte: both placements produce the same JSON.
    assert_eq!(
        stderr_before, stderr_after,
        "flag position must not affect the JSON envelope"
    );
}

// ---------------------------------------------------------------------------
// Task 9 (plan `docs/plans/tomlctl-capability-gaps.md`): `--strict-read` on
// every read subcommand — surface `kind=not_found` on a missing file instead
// of returning an empty default. Today the only read path with a "missing →
// silent default" branch is `items next-id --prefix <P>` (returns `"<P>1"`);
// every other read subcommand already errors on a missing file via
// `read_toml`'s T8-tagged NotFound, so `--strict-read` is a no-op there but
// accepted uniformly so callers can pass it without branching on subcommand.
//
// Default (flag absent) behaviour must stay byte-identical to pre-T9:
// `items next-id --prefix R <missing>` still mints `"R1"` for flows that
// bootstrap the ledger lazily. Pinned in `items_next_id_on_missing_file_prints_prefix_one`
// above; the (a) test below re-asserts it for the T9 section's completeness.
//
// Layering: `--strict-read` fires BEFORE `--verify-integrity`, so
// `items list <missing> --strict-read --verify-integrity` produces
// `kind=not_found`, NOT `kind=integrity`. This is the ordering the README's
// "File state contract" subsection guarantees.
// ---------------------------------------------------------------------------

/// T9 (a): default (flag absent) behaviour on `items next-id` with a missing
/// ledger stays byte-identical to pre-T9 — `"R1"` is the R19 bootstrapping
/// fast path, and nothing about the T9 addition is allowed to disturb it.
/// Duplicates `items_next_id_on_missing_file_prints_prefix_one` in spirit
/// but lives in the T9 section so a regression in the strict-read gate
/// surfaces alongside the T9 tests instead of in the far-away R58 block.
#[test]
fn strict_read_default_preserves_next_id_missing_file_fast_path() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-ledger.toml");
    assert!(!missing.exists());

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("next-id")
        .arg(&missing)
        .arg("--prefix")
        .arg("R")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains("\"R1\""),
        "default (non-strict) next-id on missing file must still mint \"R1\", got:\n{stdout}"
    );
}

/// T9 (b): `--strict-read` on a missing-file `items next-id` errors with the
/// documented "file does not exist" prose on stderr and exits 1. Without the
/// flag the command succeeds with `"R1"` (covered above).
#[test]
fn strict_read_next_id_missing_file_errors_with_not_found_prose() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-ledger.toml");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("next-id")
        .arg(&missing)
        .arg("--prefix")
        .arg("R")
        .arg("--strict-read")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("file does not exist:"),
        "stderr must carry the T9 not_found prose, got:\n{stderr}"
    );
    assert!(
        stderr.contains("no-such-ledger.toml"),
        "stderr must name the missing path, got:\n{stderr}"
    );
}

/// T9 (c): `--strict-read` composes with `--error-format json` — the stderr
/// envelope's `error.kind` is `"not_found"` and the `file` field is populated
/// with the missing path. Uses `items list` to cover the "benign no-op"
/// dispatch arm: today `items list` already errors on a missing file via
/// `read_toml`, so `--strict-read` doesn't change the outcome there, but it
/// MUST still surface `kind=not_found` through the T9 gate (rather than
/// letting `read_toml`'s own NotFound win, which would be behaviourally
/// identical but bypass the T9 ordering contract in (d) below).
#[test]
fn strict_read_items_list_missing_file_json_envelope_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-ledger.toml");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("list")
        .arg(&missing)
        .arg("--strict-read")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], serde_json::json!("not_found"));
    let message = err["message"].as_str().unwrap();
    assert!(
        message.contains("file does not exist:"),
        "message must be the T9 strict-read prose, got: {message}"
    );
    let file_field = err["file"].as_str().expect("file must be populated");
    assert!(
        file_field.contains("no-such-ledger.toml"),
        "file field must carry the missing path, got: {file_field}"
    );
}

/// T9 (d): layering — `--strict-read` fires BEFORE `--verify-integrity`.
/// A missing file under both flags surfaces `kind=not_found`, NOT
/// `kind=integrity`, even though the sidecar verify would also have failed
/// (the sidecar is trivially missing too). Pins the ordering documented in
/// the README's "File state contract" subsection so a future refactor that
/// reordered `strict_read_check` past `maybe_verify_integrity` trips this
/// test rather than silently reclassifying the error.
#[test]
fn strict_read_fires_before_verify_integrity_on_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-ledger.toml");

    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("list")
        .arg(&missing)
        .arg("--strict-read")
        .arg("--verify-integrity")
        .arg("--error-format")
        .arg("json")
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = parse_json_error_envelope(&stderr);
    assert_eq!(
        err["kind"],
        serde_json::json!("not_found"),
        "strict-read must win over verify-integrity on a missing file"
    );
    let message = err["message"].as_str().unwrap();
    assert!(
        !message.contains("sidecar") && !message.contains("integrity check failed"),
        "message must be the not_found prose, not an integrity-sidecar message, got: {message}"
    );
}

/// T9 (e): `--strict-read` is accepted on every read subcommand and emits
/// a consistent `kind=not_found` envelope. Spot-check `parse`, `get`,
/// `validate`, `items get`, `items orphans`, and `items find-duplicates`
/// — each is a different dispatch arm that flattens `ReadIntegrityArgs`.
/// A single array-driven test keeps the arity manageable and pins the
/// uniform surface without bloating the test count.
#[test]
fn strict_read_uniform_across_read_subcommands() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-ledger.toml");

    // Each entry is the argv after the binary name. `--strict-read`
    // `--error-format json` are appended inside the loop so the test
    // body reads flat.
    let cases: &[&[&str]] = &[
        &["parse", ""],
        &["get", "", "some.path"],
        &["validate", ""],
        &["items", "get", "", "R1"],
        &["items", "orphans", ""],
        &["items", "find-duplicates", ""],
    ];

    for argv in cases {
        let mut cmd = Command::cargo_bin("tomlctl").unwrap();
        cmd.env("TOMLCTL_ROOT", dir.path());
        // Replace the empty-string placeholder with the missing path. The
        // argv shape above pins placement (file arg is always the first
        // empty string) so a future subcommand added with a different
        // layout would need an explicit entry.
        for a in *argv {
            if a.is_empty() {
                cmd.arg(&missing);
            } else {
                cmd.arg(a);
            }
        }
        cmd.arg("--strict-read")
            .arg("--error-format")
            .arg("json")
            .write_stdin("");
        let out = cmd.assert().failure().code(1);
        let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
        let err = parse_json_error_envelope(&stderr);
        assert_eq!(
            err["kind"],
            serde_json::json!("not_found"),
            "subcommand {:?} must surface kind=not_found under --strict-read, got envelope: {err}",
            argv
        );
    }
}

// ---------------------------------------------------------------------------
// Task 2 (plan `docs/plans/tomlctl-capability-gaps.md`): `--raw` bare-scalar
// output for `items list --count` / `--count-distinct` / `--pluck` (N=1 or
// `--lines`-streamed) and for `get <file> <scalar-path>`. The motivation is
// the ~35 `tomlctl ... | jq -r .count` pipe chains the transcript audit
// uncovered: agents consuming counts or single-scalar `get` results into a
// bash `read -r N` loop want the bare integer/string on stdout, not the
// JSON-wrapped form. Error strings on invalid compositions are load-bearing
// — tests assert byte-for-byte — so a downstream script checking for an
// exact substring stays stable across releases.
// ---------------------------------------------------------------------------

/// T2-1: `items list --count --raw` emits a bare integer plus a single
/// trailing newline. Byte-identity check — the whole point of `--raw` is
/// that the stdout is parseable by `read -r N` without jq.
#[test]
fn items_list_count_raw_emits_bare_integer() {
    let stdout = run_list_query(&["--count", "--raw"]);
    assert_eq!(stdout, "6\n", "QUERY_FIXTURE has 6 rows; expected bare `6\\n`");
}

/// T2-2: `items list --count-distinct foo --raw` emits the bare count,
/// dropping the `field` key. Stdout is a single integer line with no
/// JSON wrapping.
#[test]
fn items_list_count_distinct_raw_emits_bare_integer() {
    let stdout = run_list_query(&["--count-distinct", "category", "--raw"]);
    // QUERY_FIXTURE categories: style, bug, bug, perf, style, security → 4.
    assert_eq!(stdout, "4\n", "expected bare `4\\n`; got:\n{stdout}");
}

/// T2-3: `--pluck foo --raw` with N=1 (string) emits the unquoted string.
/// Uses the `symbol` field from QUERY_FIXTURE which only R2 carries.
#[test]
fn items_list_pluck_raw_n_eq_1_string_emits_unquoted() {
    let stdout = run_list_query(&["--where-has", "symbol", "--pluck", "symbol", "--raw"]);
    // QUERY_FIXTURE R2 has symbol = "old::fn".
    assert_eq!(stdout, "old::fn\n", "expected bare `old::fn\\n`; got:\n{stdout}");
}

/// T2-4: `--pluck foo --raw` with N=1 (integer) emits the bare integer.
/// Exercise the JsonValue::Number arm of `emit_raw` with a genuine integer
/// coming out of toml's `Integer` type.
#[test]
fn items_list_pluck_raw_n_eq_1_integer_emits_bare() {
    // Use `--where id=R1` + `--pluck rounds` — but QUERY_FIXTURE doesn't
    // carry `rounds`. Build a one-row fixture instead.
    let fixture = r#"schema_version = 1

[[items]]
id = "R1"
n = 42
"#;
    let stdout = run_list_query_with(fixture, &["--pluck", "n", "--raw"]);
    assert_eq!(stdout, "42\n", "expected bare `42\\n`; got:\n{stdout}");
}

/// T2-5: `--pluck foo --raw` on a 0-item result errors with the exact
/// task-spec wording. Tests assert byte-for-byte — a reword to
/// "no items matched" or "empty result" would break agent scripts.
#[test]
fn items_list_pluck_raw_n_eq_0_errors_with_exact_message() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        // Nothing matches `status=absent` → 0 rows → 0 plucked values.
        .arg("--where")
        .arg("status=absent")
        .arg("--pluck")
        .arg("id")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("--raw requires single-value output (got 0 items)"),
        "exact error string expected; got stderr:\n{stderr}"
    );
}

/// T2-6: `--pluck foo --raw` on N>1 rows errors with the pinned wording
/// (including the suggested `--lines` remediation). Substitutes the
/// actual N in — asserts on the literal `(got 6 items)` so a drift in
/// count arithmetic would be caught.
#[test]
fn items_list_pluck_raw_n_gt_1_errors_with_exact_message_and_count() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--pluck")
        .arg("id")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains(
            "--raw requires single-value output (got 6 items); use --lines for newline-delimited"
        ),
        "exact error string expected; got stderr:\n{stderr}"
    );
}

/// T2-7: `--pluck foo --raw --lines` emits one bare value per line. The
/// streaming path threads `q.raw` through to the per-item emit point,
/// so strings come out unquoted. Pin the byte sequence to catch any
/// regression that accidentally re-quotes.
#[test]
fn items_list_pluck_raw_with_lines_emits_bare_per_line() {
    let stdout = run_list_query_with(PLUCK_FIXTURE, &["--pluck", "x", "--raw", "--lines"]);
    assert_eq!(stdout, "v1\nv2\nv3\nv4\n", "expected 4 bare lines; got:\n{stdout}");
}

/// T2-8: `tomlctl get <file> <scalar-path> --raw` emits the bare value on
/// a scalar target (integer here). Covers the `Cmd::Get` raw branch.
#[test]
fn get_raw_on_integer_scalar_emits_bare_integer() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let doc = claude.join("context.toml");
    fs::write(&doc, "[tasks]\ntotal = 7\nname = \"launch\"\n").unwrap();
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("get")
        .arg(&doc)
        .arg("tasks.total")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert_eq!(stdout, "7\n", "expected bare `7\\n`; got:\n{stdout}");
}

/// T2-9: `get <file> <table-path> --raw` errors with the exact wording the
/// task spec pins. `[tasks]` is a TOML table, so navigating to `tasks`
/// returns a JSON object — `emit_raw` rejects it.
#[test]
fn get_raw_on_table_errors_with_exact_message() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let doc = claude.join("context.toml");
    fs::write(&doc, "[tasks]\ntotal = 7\n").unwrap();
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("get")
        .arg(&doc)
        .arg("tasks")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("--raw requires a scalar target; got table"),
        "exact error string expected; got stderr:\n{stderr}"
    );
}

/// T2-10: `get <file> <array-path> --raw` errors with the exact wording.
/// `scope` below is a TOML array.
#[test]
fn get_raw_on_array_errors_with_exact_message() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let doc = claude.join("context.toml");
    fs::write(&doc, "scope = [\"a\", \"b\"]\n").unwrap();
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("get")
        .arg(&doc)
        .arg("scope")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("--raw requires a scalar target; got array"),
        "exact error string expected; got stderr:\n{stderr}"
    );
}

/// T2-11: `items list --count-by foo --raw` is rejected at `validate_query`
/// with the exact canonical message. `--count-by` emits a map, which has
/// no bare-scalar form.
#[test]
fn items_list_count_by_with_raw_errors_with_exact_message() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--count-by")
        .arg("status")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains(
            "--raw is not supported on --count-by / --group-by (output is a map, not a scalar)"
        ),
        "exact error string expected; got stderr:\n{stderr}"
    );
}

/// T2-12: same error for `--group-by foo --raw`. Pins that validation hits
/// both shapes — not just CountBy by accident.
#[test]
fn items_list_group_by_with_raw_errors_with_exact_message() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--group-by")
        .arg("status")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains(
            "--raw is not supported on --count-by / --group-by (output is a map, not a scalar)"
        ),
        "exact error string expected; got stderr:\n{stderr}"
    );
}

/// T2-13: `--pluck foo --distinct --raw` — distinct narrows the pluck
/// array to 1 row; raw then emits that lone bare value. Covers the
/// interaction between the pluck-field dedup path and the N==1 raw
/// happy case, which has a non-obvious code path (dedup runs in the
/// slow path of `run()` since `--distinct` is engaged).
#[test]
fn items_list_pluck_distinct_raw_n_eq_1_emits_bare() {
    // Fixture has four identical x values — dedup collapses to one.
    let fixture = r#"schema_version = 1

[[items]]
id = "R1"
x = "only"

[[items]]
id = "R2"
x = "only"

[[items]]
id = "R3"
x = "only"
"#;
    let stdout = run_list_query_with(fixture, &["--pluck", "x", "--distinct", "--raw"]);
    assert_eq!(stdout, "only\n", "expected bare `only\\n`; got:\n{stdout}");
}

/// T2-14: `--count --raw --error-format json` on a HAPPY path emits the
/// bare integer on stdout — the `--error-format json` flag only affects
/// errors. Pins that `--raw` output is NOT JSON-wrapped just because the
/// error-format is `json`.
#[test]
fn items_list_count_raw_with_error_format_json_still_bare_on_happy_path() {
    let (dir, ledger) = seed_ledger(QUERY_FIXTURE);
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("--error-format")
        .arg("json")
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--count")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert_eq!(
        stdout, "6\n",
        "happy-path --raw stdout must be bare; --error-format json only affects errors; got:\n{stdout}"
    );
}

/// T2-15: `--strict-read` wins against `--raw`-N=0: a missing ledger must
/// surface `kind=not_found`, NOT the "(got 0 items)" raw-validation error,
/// because the strict-read gate fires BEFORE the query pipeline runs.
/// Tests the documented ordering contract from T9.
#[test]
fn items_list_pluck_raw_strict_read_on_missing_file_wins() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join(".claude").join("no-ledger.toml");
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("--error-format")
        .arg("json")
        .arg("items")
        .arg("list")
        .arg(&missing)
        .arg("--pluck")
        .arg("id")
        .arg("--raw")
        .arg("--strict-read")
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let envelope: serde_json::Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("json-mode stderr must parse: {e}; stderr:\n{stderr}"));
    assert_eq!(
        envelope
            .get("error")
            .and_then(|e| e.get("kind"))
            .and_then(|s| s.as_str()),
        Some("not_found"),
        "strict-read must surface kind=not_found (not raw-validation); got stderr:\n{stderr}"
    );
}

/// T2-16: `--pluck foo --raw` with N=1 boolean emits `true` / `false` bare.
/// Covers the JsonValue::Bool arm of `emit_raw`.
#[test]
fn items_list_pluck_raw_n_eq_1_bool_emits_true() {
    let fixture = r#"schema_version = 1

[[items]]
id = "R1"
active = true
"#;
    let stdout = run_list_query_with(fixture, &["--pluck", "active", "--raw"]);
    assert_eq!(stdout, "true\n", "expected bare `true\\n`; got:\n{stdout}");
}

// ---------------------------------------------------------------------------
// Task 7 (plan `docs/plans/tomlctl-capability-gaps.md`): `tomlctl
// capabilities` emits a JSON description of the binary's user-facing
// surface so downstream flow-command templates can feature-gate cleanly
// without parsing `--help` prose. Also pins the 0.1.0 → 0.2.0 version
// bump that this minor release carries (new flags, new subcommand,
// auto-populated `dedup_id` field, structured `--error-format json`).
// ---------------------------------------------------------------------------

/// T7-1: `tomlctl capabilities` writes a JSON object to stdout with the
/// three top-level keys the spec pins (`version`, `features`, `subcommands`).
/// Also asserts it parses cleanly as JSON — no trailing garbage, no BOM.
#[test]
fn capabilities_output_parses_as_json() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("capabilities")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("capabilities stdout must parse as JSON: {e}; stdout:\n{stdout}"));
    let obj = v
        .as_object()
        .expect("capabilities output must be a JSON object");
    assert!(obj.contains_key("version"), "missing `version` key: {v}");
    assert!(obj.contains_key("features"), "missing `features` key: {v}");
    assert!(
        obj.contains_key("subcommands"),
        "missing `subcommands` key: {v}"
    );
}

/// T7-2: the `features` array advertises every T1..T11 feature the plan
/// enumerated. The expected list duplicates the names from `cli::FEATURES`
/// deliberately — if the const drifts (someone removes a feature or renames
/// one), this test fails in review rather than silently shipping a
/// half-advertised capability set.
#[test]
fn capabilities_features_contains_every_plan_feature() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("capabilities")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("capabilities stdout must parse as JSON: {e}"));
    let features = v
        .get("features")
        .and_then(|f| f.as_array())
        .expect("`features` must be a JSON array");
    let features: Vec<&str> = features.iter().filter_map(|e| e.as_str()).collect();

    // Exhaustive list duplicated from cli::FEATURES — drift between the
    // two is caught here in review.
    let expected = [
        "count_distinct",         // T1
        "raw",                    // T2
        "lines",                  // T3
        "infer_prefix",           // T4
        "dedupe_by",              // T5
        "dedup_id_auto",          // T6b
        "find_duplicates_across", // T6c
        "capabilities",           // T7
        "error_format_json",      // T8
        "strict_read",            // T9
        "dry_run",                // T10
        "backfill_dedup_id",      // T11
        "integrity_refresh",      // sidecar bootstrap / recovery primitive
    ];
    for name in expected {
        assert!(
            features.contains(&name),
            "expected feature `{name}` in capabilities output; got {features:?}"
        );
    }
    assert_eq!(
        features.len(),
        expected.len(),
        "feature count drift: expected {} entries, got {} ({features:?})",
        expected.len(),
        features.len()
    );
}

/// T7-3: the `version` string equals `0.3.0`. Literal assertion rather
/// than reading Cargo.toml — the whole point of this task is the semver
/// bump, so pinning the exact release marker keeps the acceptance criterion
/// honest. Bump both sides in lockstep on the next minor release.
#[test]
fn capabilities_version_matches_cargo_toml() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("capabilities")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("capabilities stdout must parse as JSON: {e}"));
    let version = v
        .get("version")
        .and_then(|s| s.as_str())
        .expect("`version` must be a string");
    assert_eq!(
        version, "0.3.0",
        "expected version `0.3.0` (the 0.2.0 → 0.3.0 bump for the new integrity subcommand); got `{version}`"
    );
}

/// T7-4: the `subcommands` array includes the metadata subcommand itself
/// (so `tomlctl capabilities | jq '.subcommands | index("capabilities")'`
/// is truthy) plus at least one real data-path subcommand (`items`). Both
/// sanity-check the list is populated and not an empty placeholder.
#[test]
fn capabilities_subcommands_contains_capabilities_and_items() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("capabilities")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("capabilities stdout must parse as JSON: {e}"));
    let subs = v
        .get("subcommands")
        .and_then(|s| s.as_array())
        .expect("`subcommands` must be a JSON array");
    let subs: Vec<&str> = subs.iter().filter_map(|e| e.as_str()).collect();
    assert!(
        subs.contains(&"capabilities"),
        "subcommands must include `capabilities`; got {subs:?}"
    );
    assert!(
        subs.contains(&"items"),
        "subcommands must include `items`; got {subs:?}"
    );
}

// ---------------------------------------------------------------------------
// R44: plan T7 acceptance requires a `--help` snapshot test for each new
// flag introduced by tasks T1..T11. The existing
// `lines_flag_listed_in_items_list_help` test (T3) establishes the shape:
// invoke the relevant subcommand with `--help`, then `assert!(stdout.contains("--flag"))`.
// The tests below clone that shape for every other T1..T11 flag, so a clap
// refactor that silently drops or hides a flag fails here in CI rather
// than during an agent-facing invocation. `--error-format` is the only
// truly global flag (it attaches at the top-level clap::Parser), so it is
// asserted against `tomlctl --help`. The other "global-ish" flags
// (`--strict-read`, `--dry-run`) are flattened into ReadIntegrityArgs /
// WriteIntegrityArgs which hang off specific subcommands — the tests pick
// one representative subcommand where each flag is surfaced.
// ---------------------------------------------------------------------------

/// R44 (T1): `items list --help` lists `--count-distinct` as a discrete
/// flag. Paired with `lines_flag_listed_in_items_list_help` to pin every
/// T1..T3 `items list` flag against clap drift.
#[test]
fn count_distinct_flag_listed_in_items_list_help() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("items")
        .arg("list")
        .arg("--help")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains("--count-distinct"),
        "items list --help must list --count-distinct as a discrete flag; got:\n{stdout}"
    );
}

/// R44 (T2): `items list --help` lists `--raw` as a discrete flag. `--raw`
/// also appears on `get --help`, but `items list` is the primary surface
/// T2 targets (bare-scalar output for counts / single-pluck).
#[test]
fn raw_flag_listed_in_items_list_help() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("items")
        .arg("list")
        .arg("--help")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains("--raw"),
        "items list --help must list --raw as a discrete flag; got:\n{stdout}"
    );
}

/// R44 (T4): `items next-id --help` lists `--infer-from-file` as a
/// discrete flag. T4 makes `--prefix` and `--infer-from-file` a required
/// mutex via an ArgGroup — clap renders both as unadorned in the usage
/// line, so a substring match on the flag name is the stable assertion.
#[test]
fn infer_from_file_flag_listed_in_items_next_id_help() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("items")
        .arg("next-id")
        .arg("--help")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains("--infer-from-file"),
        "items next-id --help must list --infer-from-file as a discrete flag; got:\n{stdout}"
    );
}

/// R44 (T5): `items add --help` lists `--dedupe-by` as a discrete flag.
/// The same flag is also defined on `items add-many`; `add` is chosen as
/// the representative surface because it is the single-item path agents
/// reach for first.
#[test]
fn dedupe_by_flag_listed_in_items_add_help() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("items")
        .arg("add")
        .arg("--help")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains("--dedupe-by"),
        "items add --help must list --dedupe-by as a discrete flag; got:\n{stdout}"
    );
}

/// R44 (T6c): `items find-duplicates --help` lists `--across` as a
/// discrete flag. Cross-ledger duplicate detection is the sole surface
/// `--across` attaches to.
#[test]
fn across_flag_listed_in_items_find_duplicates_help() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("items")
        .arg("find-duplicates")
        .arg("--help")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains("--across"),
        "items find-duplicates --help must list --across as a discrete flag; got:\n{stdout}"
    );
}

/// R44 (T8): top-level `tomlctl --help` lists `--error-format` as a
/// discrete flag. `--error-format` is defined on the top-level Cli struct
/// with `global = true`, so it renders in the root help block — the
/// natural surface for a stderr-format selector that predates any
/// subcommand dispatch.
#[test]
fn error_format_flag_listed_in_top_level_help() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("--help")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains("--error-format"),
        "tomlctl --help must list --error-format as a discrete global flag; got:\n{stdout}"
    );
}

/// R44 (T9): `items next-id --help` lists `--strict-read` as a discrete
/// flag. `--strict-read` lives on `ReadIntegrityArgs` which is flattened
/// into every read subcommand; `next-id` is the representative surface
/// because the flag's documented purpose — errorring on a missing ledger
/// instead of minting `<prefix>1` — is specifically about `next-id`'s
/// bootstrapping fast path.
#[test]
fn strict_read_flag_listed_in_items_next_id_help() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("items")
        .arg("next-id")
        .arg("--help")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains("--strict-read"),
        "items next-id --help must list --strict-read as a discrete flag; got:\n{stdout}"
    );
}

/// R44 (T10): `items remove --help` lists `--dry-run` as a discrete flag.
/// `--dry-run` is defined on three T10/T11 subcommands (`items remove`,
/// `items apply`, `items backfill-dedup-id`); `remove` is chosen as the
/// representative surface because it is the smallest command and the one
/// most likely to be invoked ad-hoc where a preview matters.
#[test]
fn dry_run_flag_listed_in_items_remove_help() {
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .arg("items")
        .arg("remove")
        .arg("--help")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains("--dry-run"),
        "items remove --help must list --dry-run as a discrete flag; got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// R45: plan T11 acceptance (d) — the end-to-end contract linking
// `items backfill-dedup-id` to the T1 query surface. Seed a ledger with
// N items that are all missing `dedup_id` (via `TOMLCTL_NO_DEDUP_ID=1`
// during seeding to defeat the auto-populate on `add`), run backfill,
// then assert the T1 surface reports N distinct dedup_id values.
//
// The plan spec literally writes the verification query as
// `items list --pluck dedup_id --distinct --count-distinct dedup_id --raw`,
// but `--pluck` and `--count-distinct` live in the same clap `shape`
// ArgGroup (cli.rs:621) and are mutually exclusive. The achievable T1
// equivalent that preserves the plan's intent — "confirm N distinct
// dedup_id values are present on the T1 surface after backfill" — is
// `items list --count-distinct dedup_id --raw`, which is tested below.
// ---------------------------------------------------------------------------

/// R45 / T11 (d): backfill + T1 surface — after `items backfill-dedup-id`
/// on an N-item legacy ledger, `items list --count-distinct dedup_id
/// --raw` reports exactly N. This pins the plan-level contract that the
/// backfill path populates *distinct* dedup_id values (not duplicates)
/// on every item, visible from the agent-facing query surface.
#[test]
fn items_backfill_dedup_id_then_count_distinct_reports_n() {
    let (dir, ledger) = seed_ledger("schema_version = 1\n");
    // Seed three items with the kill switch engaged so the add path
    // skips auto-populate, giving us a legacy-shaped ledger (identical
    // seeding pattern to items_backfill_dedup_id_populates_every_missing_item).
    for (id, summary) in &[("R1", "alpha"), ("R2", "beta"), ("R3", "gamma")] {
        Command::cargo_bin("tomlctl")
            .unwrap()
            .env("TOMLCTL_ROOT", dir.path())
            .env("TOMLCTL_LOCK_TIMEOUT", "5")
            .env("TOMLCTL_NO_DEDUP_ID", "1")
            .arg("items")
            .arg("add")
            .arg(&ledger)
            .arg("--json")
            .arg(format!(
                r#"{{"id":"{id}","file":"src/a.rs","summary":"{summary}","severity":"warning","category":"quality"}}"#,
            ))
            .write_stdin("")
            .assert()
            .success();
    }

    // Backfill — kill switch off, every item gets a dedup_id.
    Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .env_remove("TOMLCTL_NO_DEDUP_ID")
        .arg("items")
        .arg("backfill-dedup-id")
        .arg(&ledger)
        .write_stdin("")
        .assert()
        .success();

    // T1 surface: count of distinct dedup_id values must equal N (3).
    // `--raw` emits the bare integer plus a trailing newline, byte-identical
    // to the existing items_list_count_distinct_raw_emits_bare_integer test.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", dir.path())
        .arg("items")
        .arg("list")
        .arg(&ledger)
        .arg("--count-distinct")
        .arg("dedup_id")
        .arg("--raw")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert_eq!(
        stdout, "3\n",
        "after backfill, --count-distinct dedup_id --raw must report exactly N=3; got:\n{stdout}"
    );
}

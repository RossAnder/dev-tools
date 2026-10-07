//! Every stdout write goes through `output.rs`, so the global output options
//! reach every command; and the clap tree carries no duplicate long name,
//! which a global flag colliding with a per-command one would introduce.

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

/// Assembled from pieces so this file's own source never matches it.
fn stdout_write() -> Regex {
    let print = concat!("print", "(ln)?", "!", r"\(");
    let stdout = concat!("std", "out", r"\(", r"\)");
    Regex::new(&format!(r"(?-u:\b)(?:{print}|{stdout})")).unwrap()
}

/// Paths under `src/` the scan leaves out: the output layer itself and code
/// that only ever runs under test.
fn exempt(rel: &str) -> bool {
    rel == "output.rs"
        || rel.starts_with("output/")
        || rel.starts_with("cli/dispatch/tests/")
        || rel == "test_support.rs"
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The lines before the file's test module: a `#[cfg(test)]` directly
/// followed by `mod tests`. A `#[cfg(test)]` on anything else gates one item
/// mid-file and leaves the rest of the file in scope.
fn scanned_lines(text: &str) -> Vec<(usize, &str)> {
    let lines: Vec<&str> = text.lines().collect();
    let end = lines
        .windows(2)
        .position(|w| w[0].trim() == "#[cfg(test)]" && w[1].trim_start().starts_with("mod tests"))
        .unwrap_or(lines.len());
    lines[..end]
        .iter()
        .enumerate()
        .map(|(i, l)| (i + 1, *l))
        .collect()
}

fn violations(src: &Path) -> Vec<String> {
    let pattern = stdout_write();
    let mut files = Vec::new();
    rust_files(src, &mut files);
    files.sort();
    let mut found = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if exempt(&rel) {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        for (n, line) in scanned_lines(&text) {
            if pattern.is_match(line) {
                found.push(format!("src/{rel}:{n}: {}", line.trim()));
            }
        }
    }
    found
}

#[test]
fn stdout_is_written_only_by_the_output_module() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let found = violations(&src);
    assert!(
        found.is_empty(),
        "stdout written outside `output.rs`, where the global output options \
         cannot reach it — route it through an `output` emitter:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_pattern_is_word_bounded_and_spares_stderr() {
    let p = stdout_write();
    for hit in [
        "    println!(\"{x}\");",
        "print!(\"x\")",
        "let out = std::io::stdout();",
        "io::stdout().lock()",
    ] {
        assert!(p.is_match(hit), "{hit}");
    }
    for miss in [
        "eprintln!(\"{x}\");",
        "eprint!(\"x\")",
        "std::io::stderr()",
        "fn my_stdout() {}",
        "reprint!(x)",
    ] {
        assert!(!p.is_match(miss), "{miss}");
    }
}

#[test]
fn only_a_test_module_ends_the_scan() {
    let text = "#[cfg(test)]\nuse helper;\nprintln!(\"a\");\n#[cfg(test)]\nmod tests {\n    println!(\"b\");\n}\n";
    let lines: Vec<usize> = scanned_lines(text).into_iter().map(|(n, _)| n).collect();
    assert_eq!(lines, [1, 2, 3]);
    let text = "#[cfg(test)]\nmod test_support;\nprintln!(\"a\");\n";
    assert_eq!(scanned_lines(text).len(), 3);
}

#[test]
fn the_command_tree_has_no_duplicate_flags() {
    crate::test_support::on_cli_stack(|| {
        use clap::CommandFactory as _;
        crate::cli::Cli::command().debug_assert();
    });
}

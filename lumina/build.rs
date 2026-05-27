//! Build hook: produce `web/dist/` before rustc reaches the `rust-embed` macro
//! in `src/assets.rs`. Runs on every `cargo build` (debug + release), but the
//! `cargo:rerun-if-changed` lines below scope re-execution to actual SPA
//! source changes — pure Rust iteration does not re-invoke bun.
//!
//! Escape hatch: `LUMINA_SKIP_WEB_BUILD=1` skips bun entirely and drops a
//! minimal placeholder `web/dist/index.html` so the rust-embed macro can still
//! compile. Use this on hosts without bun (some CI lanes) or for fast
//! Rust-only rebuilds when you know the existing dist/ is fine to keep.

use std::env;
use std::path::Path;
use std::process::Command;

const SKIP_ENV: &str = "LUMINA_SKIP_WEB_BUILD";

fn main() {
    // Re-run only when SPA inputs change. node_modules/ and dist/ are
    // deliberately excluded — watching them would cause an infinite rebuild
    // loop (this script writes into dist/).
    for path in [
        "web/src",
        "web/index.html",
        "web/public",
        "web/package.json",
        "web/bun.lock",
        "web/vite.config.ts",
        "web/tsconfig.json",
        "web/tsconfig.app.json",
        "web/tsconfig.node.json",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-env-changed={SKIP_ENV}");

    let web_dir = Path::new("web");

    if env::var_os(SKIP_ENV).is_some() {
        println!("cargo:warning={SKIP_ENV} set — skipping SPA build");
        ensure_placeholder_index(web_dir);
        return;
    }

    if !web_dir.join("node_modules").exists() {
        run(web_dir, "bun", &["install", "--frozen-lockfile"]);
    }
    run(web_dir, "bun", &["run", "build"]);
}

fn run(cwd: &Path, cmd: &str, args: &[&str]) {
    let status = Command::new(cmd)
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap_or_else(|e| {
            panic!(
                "failed to spawn `{cmd}` in {}: {e}\n\
                 hint: install bun (https://bun.sh) or set {SKIP_ENV}=1 to skip the SPA build",
                cwd.display()
            )
        });
    if !status.success() {
        panic!("`{cmd} {}` exited with {status}", args.join(" "));
    }
}

fn ensure_placeholder_index(web_dir: &Path) {
    let dist = web_dir.join("dist");
    let index = dist.join("index.html");
    if index.exists() {
        return;
    }
    std::fs::create_dir_all(&dist).expect("create web/dist");
    std::fs::write(
        &index,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>lumina</title></head>\
         <body><div id=\"app\"></div></body></html>\n",
    )
    .expect("write placeholder web/dist/index.html");
}

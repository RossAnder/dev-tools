// user32.dll is needed only once the TUI parses a key (crossterm's key
// translation calls GetForegroundWindow, GetKeyboardLayout,
// GetWindowThreadProcessId and ToUnicodeEx). Delay-loading it keeps it off the
// start-up path of the `glimpse hook` and `--once` processes, which never
// reach that code.
fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    let os = std::env::var("CARGO_CFG_TARGET_OS");
    let env = std::env::var("CARGO_CFG_TARGET_ENV");
    if os.as_deref() == Ok("windows") && env.as_deref() == Ok("msvc") {
        println!("cargo::rustc-link-arg-bins=/DELAYLOAD:user32.dll");
        println!("cargo::rustc-link-arg-bins=delayimp.lib");
    }
}

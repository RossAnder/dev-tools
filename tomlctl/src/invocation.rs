// One command's state, held in a thread-local for the length of that command:
// its stdout and stderr buffers, its stdin bytes and claimed flag, an
// environment overlay and its output options. An in-process caller installs a
// context around one run; the binary installs none. Every accessor behaves as
// the process does today when no context is installed, and an accessor whose
// no-context behaviour lives in another module returns `None` (or `false`) so
// that module keeps it. The command path is single-threaded, so one
// thread-local covers a whole command; child processes and the working
// directory are not covered.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::{IsTerminal, Read, Write};

use crate::output::OutputOpts;

/// What one in-process run is given: its stdin bytes and the environment
/// variables it sees set or unset over the process environment.
#[derive(Default)]
pub(crate) struct Invocation {
    stdin: Vec<u8>,
    env: HashMap<OsString, Option<OsString>>,
}

impl Invocation {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn stdin(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.stdin = bytes.into();
        self
    }

    pub(crate) fn env(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.insert(name.into(), Some(value.into()));
        self
    }

    /// Hide `name` from this run even when the process environment sets it.
    pub(crate) fn unset(mut self, name: impl Into<OsString>) -> Self {
        self.env.insert(name.into(), None);
        self
    }

    /// Install this context on the current thread until the guard drops,
    /// which also happens on an early return or a panic.
    pub(crate) fn install(self) -> Guard {
        let state = State {
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdin: self.stdin,
            stdin_claimed: false,
            env: self.env,
            opts: None,
        };
        let previous = CURRENT.with(|c| c.borrow_mut().replace(state));
        Guard {
            previous: Some(previous),
        }
    }
}

struct State {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdin: Vec<u8>,
    stdin_claimed: bool,
    env: HashMap<OsString, Option<OsString>>,
    opts: Option<&'static OutputOpts>,
}

thread_local! {
    static CURRENT: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Run `f` over the installed state, or return `None` with no context.
fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> Option<T> {
    CURRENT.with(|c| c.borrow_mut().as_mut().map(f))
}

/// Removes the installed context on drop, restoring whichever context was
/// installed before it.
pub(crate) struct Guard {
    previous: Option<Option<State>>,
}

/// The output a run wrote, taken out when it ends.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Captured {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

impl Guard {
    /// Remove the context and return what the run wrote to stdout and stderr.
    pub(crate) fn finish(mut self) -> Captured {
        self.restore()
            .map(|s| Captured {
                stdout: s.stdout,
                stderr: s.stderr,
            })
            .unwrap_or_default()
    }

    fn restore(&mut self) -> Option<State> {
        let previous = self.previous.take()?;
        CURRENT.with(|c| std::mem::replace(&mut *c.borrow_mut(), previous))
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.restore();
    }
}

/// True while a context is installed on this thread.
pub(crate) fn is_active() -> bool {
    CURRENT.with(|c| c.borrow().is_some())
}

/// Run `w` over the stdout buffer, or return `None` with no context. The
/// buffer is taken out for the call, so a nested write from inside `w` lands
/// after `w`'s own bytes rather than panicking on a second borrow.
pub(crate) fn with_stdout_buffer<T>(w: impl FnOnce(&mut Vec<u8>) -> T) -> Option<T> {
    let mut buf = with_state(|s| std::mem::take(&mut s.stdout))?;
    let out = w(&mut buf);
    with_state(|s| {
        buf.append(&mut s.stdout);
        s.stdout = buf;
    });
    Some(out)
}

/// Write one line to stderr: the run's buffer with a context, the process's
/// stderr without one.
pub(crate) fn stderr_line(line: impl std::fmt::Display) {
    let appended = with_state(|s| {
        // Writing to a `Vec` cannot fail.
        let _ = writeln!(s.stderr, "{line}");
    });
    if appended.is_none() {
        eprintln!("{line}");
    }
}

/// A stderr handle that writes to the run's buffer with a context and to the
/// process's stderr without one, deciding on each write.
pub(crate) struct Stderr;

pub(crate) fn stderr() -> Stderr {
    Stderr
}

impl Write for Stderr {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match with_state(|s| s.stderr.extend_from_slice(buf)) {
            Some(()) => Ok(buf.len()),
            None => std::io::stderr().write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match with_state(|_| ()) {
            Some(()) => Ok(()),
            None => std::io::stderr().flush(),
        }
    }
}

/// A reader over the run's stdin bytes with a context, which it drains, or
/// over the process's stdin without one.
pub(crate) fn stdin_reader() -> Box<dyn Read> {
    match with_state(|s| std::mem::take(&mut s.stdin)) {
        Some(bytes) => Box::new(std::io::Cursor::new(bytes)),
        None => Box::new(std::io::stdin().lock()),
    }
}

/// Whether stdin is a terminal; a run's given bytes never are.
pub(crate) fn stdin_is_terminal() -> bool {
    !is_active() && std::io::stdin().is_terminal()
}

/// Mark the run's stdin claimed and return whether it already was, or `None`
/// with no context, where the caller keeps its process-wide flag.
pub(crate) fn claim_stdin() -> Option<bool> {
    with_state(|s| std::mem::replace(&mut s.stdin_claimed, true))
}

/// `name` from the run's overlay first, then the process environment. An
/// overlay entry that unsets `name` hides the process's value.
pub(crate) fn env_var_os(name: impl AsRef<OsStr>) -> Option<OsString> {
    let name = name.as_ref();
    match with_state(|s| s.env.get(name).cloned()).flatten() {
        Some(overlaid) => overlaid,
        None => std::env::var_os(name),
    }
}

/// The run's output options: `None` with no context, where the caller keeps
/// its process-wide options, and `Some(None)` before the run set any.
pub(crate) fn opts() -> Option<Option<&'static OutputOpts>> {
    with_state(|s| s.opts)
}

/// Store the run's output options; false with no context, where the caller
/// installs them process-wide instead.
pub(crate) fn set_opts(opts: &'static OutputOpts) -> bool {
    with_state(|s| s.opts = Some(opts)).is_some()
}

/// An error that ends the command with `.0` as its exit code and prints
/// nothing, for a verb that has already written its own report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExitWith(pub(crate) i32);

impl std::fmt::Display for ExitWith {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "exit with code {}", self.0)
    }
}

impl std::error::Error for ExitWith {}

/// The exit code `err` carries, when it is an [`ExitWith`] or wraps one.
pub(crate) fn exit_code(err: &anyhow::Error) -> Option<i32> {
    err.chain()
        .find_map(|e| e.downcast_ref::<ExitWith>().map(|x| x.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VAR: &str = "TOMLCTL_INVOCATION_TEST_VAR";

    #[test]
    fn nothing_is_active_without_a_context() {
        assert!(!is_active());
        assert!(opts().is_none());
        assert!(!set_opts(Box::leak(Box::default())));
        assert_eq!(claim_stdin(), None);
        assert_eq!(with_stdout_buffer(|b| b.len()), None);
    }

    #[test]
    fn env_falls_back_to_the_process_without_a_context() {
        assert_eq!(env_var_os("PATH"), std::env::var_os("PATH"));
        assert_eq!(env_var_os(VAR), None);
    }

    #[test]
    fn the_guard_removes_the_context_on_drop() {
        {
            let _guard = Invocation::new().install();
            assert!(is_active());
        }
        assert!(!is_active());
    }

    #[test]
    fn the_guard_removes_the_context_on_a_panic() {
        let caught = std::panic::catch_unwind(|| {
            let _guard = Invocation::new().install();
            panic!("inside a run");
        });
        assert!(caught.is_err());
        assert!(!is_active());
    }

    #[test]
    fn finish_returns_both_streams() {
        let guard = Invocation::new().install();
        assert_eq!(
            with_stdout_buffer(|b| b.extend_from_slice(b"out")),
            Some(())
        );
        stderr_line("one");
        stderr().write_all(b"two\n").unwrap();
        let captured = guard.finish();
        assert!(!is_active());
        assert_eq!(captured.stdout, b"out");
        assert_eq!(captured.stderr, b"one\ntwo\n");
    }

    #[test]
    fn a_nested_stdout_write_lands_after_the_outer_one() {
        let guard = Invocation::new().install();
        with_stdout_buffer(|outer| {
            outer.extend_from_slice(b"a");
            with_stdout_buffer(|inner| inner.extend_from_slice(b"b"));
        });
        assert_eq!(guard.finish().stdout, b"ab");
    }

    #[test]
    fn a_nested_install_restores_the_outer_context() {
        let outer = Invocation::new().install();
        stderr_line("outer");
        let inner = Invocation::new().install();
        stderr_line("inner");
        assert_eq!(inner.finish().stderr, b"inner\n");
        assert!(is_active());
        assert_eq!(outer.finish().stderr, b"outer\n");
    }

    #[test]
    fn stdin_is_the_given_bytes_and_never_a_terminal() {
        let _guard = Invocation::new().stdin("payload").install();
        assert!(!stdin_is_terminal());
        let mut read = String::new();
        stdin_reader().read_to_string(&mut read).unwrap();
        assert_eq!(read, "payload");
    }

    #[test]
    fn claim_stdin_reports_the_previous_flag() {
        let _guard = Invocation::new().install();
        assert_eq!(claim_stdin(), Some(false));
        assert_eq!(claim_stdin(), Some(true));
    }

    #[test]
    fn the_overlay_sets_and_unsets_over_the_process() {
        let _guard = Invocation::new().env(VAR, "set").unset("PATH").install();
        assert_eq!(env_var_os(VAR), Some(OsString::from("set")));
        assert_eq!(env_var_os("PATH"), None);
        assert_eq!(
            env_var_os("TOMLCTL_INVOCATION_TEST_ABSENT"),
            std::env::var_os("TOMLCTL_INVOCATION_TEST_ABSENT")
        );
    }

    #[test]
    fn opts_are_held_per_context() {
        let _guard = Invocation::new().install();
        assert_eq!(opts().map(|o| o.is_some()), Some(false));
        let set: &'static OutputOpts = Box::leak(Box::default());
        assert!(set_opts(set));
        assert!(opts().flatten().is_some_and(|o| std::ptr::eq(o, set)));
    }

    #[test]
    fn exit_with_is_found_through_context() {
        let err = anyhow::Error::from(ExitWith(3)).context("verb");
        assert_eq!(exit_code(&err), Some(3));
        assert_eq!(exit_code(&anyhow::anyhow!("other")), None);
    }
}

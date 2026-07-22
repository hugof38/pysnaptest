//! Converts insta's snapshot-mismatch panics into clean Python `AssertionError`s.
//!
//! insta only exposes *panicking* assertion macros: a mismatch is signalled by
//! `panic!("snapshot assertion for '...' failed in line N")`, and its internal
//! non-panicking entry point is not part of the public API. So the only
//! supported way to wrap insta in a library is to run the assertion, catch the
//! unwind, and turn it into an exception.
//!
//! Left alone, that panic surfaces to Python as a `pyo3_runtime.PanicException`,
//! and Rust's default hook first prints a `thread '<unnamed>' panicked at ...
//! note: run with RUST_BACKTRACE=1` line to stderr. Both are noise for a Python
//! user, who just wants the diff and a normal assertion failure. We install a
//! panic hook that stays quiet while one of our assertions is running and raise
//! a plain `AssertionError` instead.

use std::any::Any;
use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Once;

use pyo3::exceptions::PyAssertionError;
use pyo3::PyResult;

thread_local! {
    /// Set while one of our snapshot assertions is running, so the panic hook
    /// knows to stay silent for the expected mismatch panic.
    static IN_ASSERTION: Cell<bool> = const { Cell::new(false) };
}

/// Installs (once) a panic hook that suppresses stderr output while one of our
/// snapshot assertions is running, and otherwise defers to the previous hook.
fn install_quiet_hook() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if !IN_ASSERTION.with(Cell::get) {
                previous(info);
            }
        }));
    });
}

/// Marks the current thread as running a snapshot assertion, clearing the flag
/// again on drop so it is reset even if the assertion unwinds.
struct AssertionGuard;

impl AssertionGuard {
    fn enter() -> Self {
        install_quiet_hook();
        IN_ASSERTION.with(|flag| flag.set(true));
        AssertionGuard
    }
}

impl Drop for AssertionGuard {
    fn drop(&mut self) {
        IN_ASSERTION.with(|flag| flag.set(false));
    }
}

/// Extracts a human-readable message from a panic payload, if it carries one.
fn panic_message(payload: &(dyn Any + Send)) -> Option<String> {
    if let Some(s) = payload.downcast_ref::<&str>() {
        Some((*s).to_string())
    } else {
        payload.downcast_ref::<String>().cloned()
    }
}

/// Builds the `AssertionError` message for a failed assertion.
///
/// insta's mismatch panic is `snapshot assertion for '...' failed in line N`,
/// where the line number points at Rust internals and is noise for a Python
/// user; that case gets a friendly hint. Any other panic (an unexpected bug)
/// keeps its original message.
fn assertion_message(snapshot_name: &str, payload: &(dyn Any + Send)) -> String {
    let raw = panic_message(payload);
    if raw
        .as_deref()
        .is_some_and(|m| m.starts_with("snapshot assertion for"))
    {
        format!(
            "snapshot '{snapshot_name}' did not match the stored value (see the diff above). \
             Update the snapshot if this change is intentional."
        )
    } else {
        raw.unwrap_or_else(|| format!("snapshot '{snapshot_name}' assertion failed"))
    }
}

/// Runs an insta assertion, turning a snapshot mismatch (which insta signals by
/// panicking) into a Python `AssertionError`. insta prints the diff to stdout
/// before it panics, so the raised error only needs to say what to do next.
pub fn run_snapshot_assertion<F: FnOnce()>(snapshot_name: &str, assertion: F) -> PyResult<()> {
    let guard = AssertionGuard::enter();
    let outcome = panic::catch_unwind(AssertUnwindSafe(assertion));
    drop(guard);

    match outcome {
        Ok(()) => Ok(()),
        Err(payload) => Err(PyAssertionError::new_err(assertion_message(
            snapshot_name,
            payload.as_ref(),
        ))),
    }
}

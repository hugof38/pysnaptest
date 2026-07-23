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

/// Classified result of running an insta snapshot assertion under our guard.
enum AssertionOutcome {
    /// The snapshot matched (or was updated).
    Matched,
    /// insta's expected mismatch panic fired; insta printed the diff to stdout.
    Mismatch,
    /// An unexpected panic (a real bug); carries its surfaced message.
    Error(String),
}

/// Runs an insta assertion under the quiet panic hook and classifies the result.
///
/// insta signals a snapshot mismatch by panicking with a message starting
/// `snapshot assertion for '...'`; that is the one panic we treat as an expected
/// outcome. Any other panic is an unexpected bug and its message is preserved.
fn run_assertion<F: FnOnce()>(snapshot_name: &str, assertion: F) -> AssertionOutcome {
    let guard = AssertionGuard::enter();
    let outcome = panic::catch_unwind(AssertUnwindSafe(assertion));
    drop(guard);

    match outcome {
        Ok(()) => AssertionOutcome::Matched,
        Err(payload) => {
            let raw = panic_message(payload.as_ref());
            if raw
                .as_deref()
                .is_some_and(|m| m.starts_with("snapshot assertion for"))
            {
                AssertionOutcome::Mismatch
            } else {
                AssertionOutcome::Error(
                    raw.unwrap_or_else(|| format!("snapshot '{snapshot_name}' assertion failed")),
                )
            }
        }
    }
}

/// Runs an insta assertion, turning a snapshot mismatch (which insta signals by
/// panicking) into a Python `AssertionError`. insta prints the diff to stdout
/// before it panics, so the raised error only needs to say what to do next.
pub fn run_snapshot_assertion<F: FnOnce()>(snapshot_name: &str, assertion: F) -> PyResult<()> {
    match run_assertion(snapshot_name, assertion) {
        AssertionOutcome::Matched => Ok(()),
        AssertionOutcome::Mismatch => Err(PyAssertionError::new_err(format!(
            "snapshot '{snapshot_name}' did not match the stored value (see the diff above). \
             Update the snapshot if this change is intentional."
        ))),
        AssertionOutcome::Error(message) => Err(PyAssertionError::new_err(message)),
    }
}

/// Runs an insta assertion but reports the outcome as a boolean instead of
/// raising on a snapshot mismatch.
///
/// Returns `Ok(true)` when the snapshot matched (or was updated) and `Ok(false)`
/// for insta's expected mismatch panic. Any other (unexpected) panic is still
/// surfaced as a Python `AssertionError`. This lets a caller enrich the failure
/// (e.g. render a readable CSV/JSON diff for a binary DataFrame snapshot) while
/// insta still writes its pending `.new` file as usual.
pub fn run_snapshot_assertion_matched<F: FnOnce()>(
    snapshot_name: &str,
    assertion: F,
) -> PyResult<bool> {
    match run_assertion(snapshot_name, assertion) {
        AssertionOutcome::Matched => Ok(true),
        AssertionOutcome::Mismatch => Ok(false),
        AssertionOutcome::Error(message) => Err(PyAssertionError::new_err(message)),
    }
}

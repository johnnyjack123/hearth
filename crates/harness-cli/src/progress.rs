//! Steps of preparing an environment (VM, gateway, instance, clone, harness),
//! reported where the user looks: the terminal for `harness run`, the chat
//! for a linked web chat.

use std::collections::HashMap;
use std::io::Write as _;
use std::sync::Mutex;
use std::time::Instant;

use harness_core::lang::{self, Lang};

/// Receives the steps of a long preparation.
pub trait Progress: Sync {
    /// A step starts.
    fn start(&self, id: &str, label: &str);
    /// The step finished.
    fn done(&self, id: &str);
    /// The step failed.
    fn failed(&self, id: &str, error: &str);
    /// Language of the labels: the terminal follows Windows, the web chat
    /// keeps German labels until the web app translates the steps itself.
    fn lang(&self) -> Lang {
        lang::current()
    }
}

/// Runs `work` as step `id` and reports it to `progress`.
pub fn step<T>(
    progress: &dyn Progress,
    id: &str,
    label: &str,
    work: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    progress.start(id, label);
    let result = work();
    match &result {
        Ok(_) => progress.done(id),
        Err(error) => progress.failed(id, &format!("{error:#}")),
    }
    result
}

/// The terminal: `label … ✓ (1.2 s)` per step, as `harness run` always did.
#[derive(Default)]
pub struct Terminal {
    started: Mutex<HashMap<String, Instant>>,
}

impl Progress for Terminal {
    fn start(&self, id: &str, label: &str) {
        self.started
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.to_owned(), Instant::now());
        print!("{label} … ");
        let _ = std::io::stdout().flush();
    }

    fn done(&self, id: &str) {
        let started = self
            .started
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(id);
        match started {
            Some(started) => println!("✓ ({:.1} s)", started.elapsed().as_secs_f32()),
            None => println!("✓"),
        }
    }

    fn failed(&self, id: &str, _error: &str) {
        self.started
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(id);
        // The caller reports the error itself, as before.
        println!("✗");
    }
}

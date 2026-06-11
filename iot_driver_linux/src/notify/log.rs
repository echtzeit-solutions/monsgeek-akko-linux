//! Daemon activity log — shared ring buffer for TUI panel and CLI --verbose.

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;

const MAX_ENTRIES: usize = 200;

/// A single log entry with timestamp.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub elapsed_ms: u64,
    pub msg: String,
}

/// Shared daemon log — push from daemon, read from TUI/CLI.
#[derive(Clone)]
pub struct DaemonLog {
    inner: Arc<Mutex<Inner>>,
    start: std::time::Instant,
    /// If true, also print to stderr (CLI --verbose mode).
    verbose: bool,
}

struct Inner {
    entries: VecDeque<LogEntry>,
}

impl DaemonLog {
    pub fn new(verbose: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                entries: VecDeque::with_capacity(MAX_ENTRIES),
            })),
            start: std::time::Instant::now(),
            verbose,
        }
    }

    /// Push a log message.
    pub fn push(&self, msg: impl Into<String>) {
        let msg = msg.into();
        let elapsed_ms = self.start.elapsed().as_millis() as u64;

        if self.verbose {
            let secs = elapsed_ms / 1000;
            let ms = elapsed_ms % 1000;
            eprintln!("[{secs:4}.{ms:03}] {msg}");
        }

        // Emit to tui-logger for the TUI log widget
        log::info!(target: "notify", "{msg}");

        // Non-blocking: try_lock to avoid stalling the daemon loop
        if let Ok(mut inner) = self.inner.try_lock() {
            if inner.entries.len() >= MAX_ENTRIES {
                inner.entries.pop_front();
            }
            inner.entries.push_back(LogEntry { elapsed_ms, msg });
        }
    }

    /// Log a summary of active notifications (verbose/CLI only — not stored in ring buffer).
    pub fn print_state(&self, notifications: &[(u64, String, String, String, i32)]) {
        if !self.verbose {
            return;
        }
        let elapsed_ms = self.start.elapsed().as_millis() as u64;
        let secs = elapsed_ms / 1000;
        let ms = elapsed_ms % 1000;
        if notifications.is_empty() {
            eprintln!("[{secs:4}.{ms:03}] (no active notifications)");
        } else {
            for (id, key, source, effect, prio) in notifications {
                eprintln!("[{secs:4}.{ms:03}]   #{id} {effect} on {key} p={prio} ({source})");
            }
        }
    }

    /// Read all entries (for TUI rendering). Non-blocking.
    pub fn entries(&self) -> Vec<LogEntry> {
        self.inner
            .try_lock()
            .map(|inner| inner.entries.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.inner
            .try_lock()
            .map(|inner| inner.entries.len())
            .unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

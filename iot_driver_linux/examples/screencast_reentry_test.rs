//! Isolated reproduction for the "screen sync doesn't restart on re-entry" bug.
//!
//! Hypothesis: ashpd caches its D-Bus connection in a process-global
//! `static SESSION: OnceLock<zbus::Connection>`. zbus (built with the `tokio`
//! feature) spawns that connection's socket-reader task onto whatever tokio
//! runtime is current when the connection is first created. `spawn_for_tui`
//! builds a *fresh ephemeral* runtime per capture and `shutdown_background()`s
//! it on teardown — so the FIRST capture binds the global connection to a
//! runtime that is then destroyed, and EVERY later capture awaits on a dead
//! socket forever.
//!
//! This test reproduces the pattern with no keyboard and no GUI picker:
//! `create_session()` is a pure D-Bus round-trip (CreateSession), so it needs a
//! live connection but never shows a dialog. We run it on two consecutive
//! ephemeral runtimes (attempt #1 and #2) exactly like the buggy code, with a
//! hard timeout so a hang is reported instead of blocking forever.
//!
//! Run: `cargo run --example screencast_reentry_test --features screen-capture`
//!
//! Expected (buggy): attempt #1 succeeds in ~tens of ms; attempt #2 TIMES OUT.
//! With the fix (shared persistent runtime): both succeed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ashpd::desktop::screencast::Screencast;

const STEP_TIMEOUT: Duration = Duration::from_secs(4);

/// One capture-negotiation attempt on a *fresh ephemeral runtime*, mirroring the
/// buggy `spawn_for_tui` lifecycle: build runtime → ashpd calls → shutdown.
fn attempt_ephemeral(label: &str) -> bool {
    println!("\n=== attempt {label}: building fresh ephemeral runtime ===");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");

    let ok = rt.block_on(run_negotiation(label));

    // Exactly what the buggy teardown does.
    println!("attempt {label}: shutdown_background() the runtime");
    rt.shutdown_background();
    ok
}

/// Run all attempts on ONE shared, never-dropped runtime (the proposed fix).
fn attempt_shared(rt: &tokio::runtime::Runtime, label: &str) -> bool {
    println!("\n=== attempt {label}: reusing shared persistent runtime ===");
    rt.block_on(run_negotiation(label))
}

async fn run_negotiation(label: &str) -> bool {
    if !step(label, "Screencast::new", Screencast::new()).await {
        return false;
    }
    // Re-create to keep ownership simple across steps.
    let sc = match tokio::time::timeout(STEP_TIMEOUT, Screencast::new()).await {
        Ok(Ok(sc)) => sc,
        _ => {
            println!("attempt {label}: Screencast::new (2) failed/hung");
            return false;
        }
    };
    let started = Instant::now();
    match tokio::time::timeout(STEP_TIMEOUT, sc.create_session()).await {
        Ok(Ok(session)) => {
            println!(
                "attempt {label}: create_session OK in {:?}",
                started.elapsed()
            );
            let _ = tokio::time::timeout(STEP_TIMEOUT, session.close()).await;
            true
        }
        Ok(Err(e)) => {
            println!("attempt {label}: create_session ERR: {e}");
            false
        }
        Err(_) => {
            println!(
                "attempt {label}: create_session HUNG ({:?} timeout) <-- dead connection",
                STEP_TIMEOUT
            );
            false
        }
    }
}

async fn step<F, T, E>(label: &str, name: &str, fut: F) -> bool
where
    F: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let start = Instant::now();
    match tokio::time::timeout(STEP_TIMEOUT, fut).await {
        Ok(Ok(_)) => {
            println!("attempt {label}: {name} OK in {:?}", start.elapsed());
            true
        }
        Ok(Err(e)) => {
            println!("attempt {label}: {name} ERR: {e}");
            false
        }
        Err(_) => {
            println!("attempt {label}: {name} HUNG (timeout) <-- dead connection");
            false
        }
    }
}

fn main() {
    // Watchdog: hard-exit if the whole test wedges, so it never blocks forever.
    let done = Arc::new(AtomicBool::new(false));
    {
        let done = done.clone();
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(40);
            while Instant::now() < deadline {
                if done.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            eprintln!("\n!!! watchdog: test wedged, hard-exiting");
            std::process::exit(2);
        });
    }

    let mode = std::env::args().nth(1).unwrap_or_default();

    if mode == "shared" {
        println!("MODE: shared persistent runtime (the production fix)");
        // The actual process-lifetime runtime used by the driver's screen-sync
        // path — never dropped, so ashpd's cached connection survives re-entry.
        let rt = iot_driver::screen_capture::pipewire_capture::portal_runtime();
        let a = attempt_shared(rt, "#1");
        let b = attempt_shared(rt, "#2");
        let c = attempt_shared(rt, "#3");
        report(&[("#1", a), ("#2", b), ("#3", c)]);
    } else {
        println!("MODE: ephemeral per-capture runtimes (reproduces the bug)");
        let a = attempt_ephemeral("#1");
        let b = attempt_ephemeral("#2");
        let c = attempt_ephemeral("#3");
        report(&[("#1", a), ("#2", b), ("#3", c)]);
    }

    done.store(true, Ordering::SeqCst);
}

fn report(results: &[(&str, bool)]) {
    println!("\n================ RESULT ================");
    for (label, ok) in results {
        println!(
            "  attempt {label}: {}",
            if *ok { "OK" } else { "FAIL/HANG" }
        );
    }
    let first_ok = results.first().map(|(_, ok)| *ok).unwrap_or(false);
    let rest_ok = results.iter().skip(1).all(|(_, ok)| *ok);
    if first_ok && !rest_ok {
        println!("\n>>> REPRODUCED: first attempt works, re-entry hangs (dead cached connection).");
    } else if results.iter().all(|(_, ok)| *ok) {
        println!("\n>>> All attempts succeeded — connection survived re-entry.");
    } else {
        println!("\n>>> Inconclusive (first attempt itself failed?). Check portal availability.");
    }
}

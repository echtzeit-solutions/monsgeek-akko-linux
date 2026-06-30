//! Screen-reactive state for the Device Info tab.
//!
//! Like the audio visualizer, this is implied by the keyboard's LED mode: when
//! it is ScreenSync (21) the host captures the screen via the XDG ScreenCast
//! portal + PipeWire, averages each frame to one color, and streams it over
//! `SET_SCREEN_COLOR`. The only control is the capture **Rate**, and a live
//! color swatch preview appears while it runs.
//!
//! The capture path is gated behind the `screen-capture` Cargo feature (it pulls
//! in `ashpd` + `pipewire`). Without it, selecting ScreenSync shows a hint.

#[cfg(feature = "screen-capture")]
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::super::App;

/// ScreenSync LED mode (host-streamed average screen color).
pub(in crate::tui) const SCREEN_SYNC: u8 = 21;

/// Pause after tearing down a portal session before opening a new one in-process.
#[cfg(feature = "screen-capture")]
const REENTRY_COOLDOWN: Duration =
    Duration::from_millis(crate::screen_capture::pipewire_capture::REENTRY_SETTLE_MS);

/// Is this LED mode the host-streamed screen visualizer?
pub(in crate::tui) fn is_screen_mode(led_mode: u8) -> bool {
    led_mode == SCREEN_SYNC
}

/// Screen-reactive state, held in [`App`].
pub(in crate::tui) struct ScreenTabState {
    /// Capture rate (Hz) — CPU/USB traffic vs fidelity.
    pub rate_hz: u32,
    pub error: Option<String>,
    /// Earliest time we may open a new portal session after the last teardown.
    #[cfg(feature = "screen-capture")]
    reentry_after: Instant,
    #[cfg(feature = "screen-capture")]
    run: Option<crate::screen_capture::TuiScreenCapture>,
}

impl Default for ScreenTabState {
    fn default() -> Self {
        Self {
            rate_hz: crate::settings::DEFAULT_RATE_HZ,
            error: None,
            #[cfg(feature = "screen-capture")]
            reentry_after: Instant::now(),
            #[cfg(feature = "screen-capture")]
            run: None,
        }
    }
}

impl ScreenTabState {
    /// Default state with a persisted capture rate applied.
    pub(in crate::tui) fn with_rate(rate_hz: u32) -> Self {
        Self {
            rate_hz,
            ..Self::default()
        }
    }

    #[cfg(feature = "screen-capture")]
    pub(in crate::tui) fn is_running(&self) -> bool {
        self.run.as_ref().is_some_and(|r| !r.is_finished())
    }

    #[cfg(not(feature = "screen-capture"))]
    pub(in crate::tui) fn is_running(&self) -> bool {
        false
    }

    /// Current averaged screen color, if capturing.
    #[cfg(feature = "screen-capture")]
    fn color(&self) -> Option<(u8, u8, u8)> {
        self.run
            .as_ref()
            .filter(|r| !r.is_finished())
            .map(|r| r.state.get_color())
    }

    #[cfg(not(feature = "screen-capture"))]
    fn color(&self) -> Option<(u8, u8, u8)> {
        None
    }
}

/// Signal the capture worker to stop. Non-blocking — safe on the Tokio runtime.
#[cfg(feature = "screen-capture")]
fn request_stop(app: &mut App) {
    if let Some(run) = &app.screen.run {
        run.signal_stop();
    }
}

/// Reap a finished worker thread. Only joins when the thread has already exited
/// (instant); never blocks the runtime waiting for portal teardown.
#[cfg(feature = "screen-capture")]
fn try_reap_run(app: &mut App) {
    let Some(run) = app.screen.run.take() else {
        return;
    };
    if !run.is_finished() {
        app.screen.run = Some(run);
        return;
    }
    if let Some(err) = run.error.lock().unwrap().take() {
        app.screen.error = Some(err);
        app.status_msg = "Screen reactive failed".to_string();
    }
    run.join();
    // Always pause before opening a new portal session — compositor teardown
    // can lag behind the worker thread exit, and skipping this when the user
    // has already switched back to ScreenSync races the restore token and
    // leaves capture dead with no tray indicator.
    app.screen.reentry_after = Instant::now() + REENTRY_COOLDOWN;
}

/// Stop screen capture if running. Call before restoring the terminal on quit.
///
/// We don't wait for the worker's full portal teardown here: the worker, once
/// signalled, sends the bounded portal Close itself, and in any case process
/// exit drops the shared D-Bus connection — which makes the compositor stop the
/// screencast. So we only give the streaming loop a brief moment to stop
/// touching the keyboard, then detach. Crucially we use NO `spawn_blocking`: an
/// abandoned blocking task would stall the main runtime's shutdown, since
/// `Runtime::drop` waits for its blocking pool to drain (that was the multi-
/// second/indefinite quit hang).
#[cfg(feature = "screen-capture")]
pub(in crate::tui) async fn shutdown_async(app: &mut App) {
    if let Some(run) = app.screen.run.take() {
        run.signal_stop();
        tokio::time::sleep(Duration::from_millis(150)).await;
        drop(run); // detach; worker + process exit stop the recording
        app.status_msg = "Screen reactive stopped".to_string();
    }
}

#[cfg(not(feature = "screen-capture"))]
pub(in crate::tui) async fn shutdown_async(_app: &mut App) {}

/// Reconcile the screen run with the current LED mode: start capturing when
/// ScreenSync is selected, stop when it isn't. Called every tick.
pub(in crate::tui) fn reconcile(app: &mut App) {
    let is_screen = is_screen_mode(app.info.led_mode);

    #[cfg(feature = "screen-capture")]
    {
        if !is_screen {
            if app.screen.run.is_some() {
                request_stop(app);
                try_reap_run(app);
                if app.screen.run.is_none() {
                    app.status_msg = "Screen reactive stopped".to_string();
                }
            }
            return;
        }

        try_reap_run(app);

        // Only start a new capture when no prior run handle remains. A run can
        // transition to "finished" just after `try_reap_run` checks it; using
        // `is_running()` here would allow replacing that unreaped handle and
        // skipping the mandatory re-entry cooldown.
        if app.screen.run.is_some() {
            return;
        }
        if Instant::now() < app.screen.reentry_after {
            return;
        }

        start(app);
    }

    #[cfg(not(feature = "screen-capture"))]
    {
        if is_screen && app.screen.error.is_none() {
            app.screen.error =
                Some("Screen reactive needs a build with --features screen-capture".to_string());
        } else if !is_screen {
            app.screen.error = None;
        }
    }
}

#[cfg(feature = "screen-capture")]
fn start(app: &mut App) {
    let Some(keyboard) = app.keyboard.clone() else {
        app.screen.error = Some("No keyboard connected".to_string());
        return;
    };

    crate::screen_capture::set_screen_sync_mode(&keyboard);

    let capture = crate::screen_capture::spawn_for_tui(keyboard, app.screen.rate_hz);
    app.screen.run = Some(capture);
    app.screen.error = None;
    app.status_msg = "Screen reactive: capturing screen".to_string();
}

/// Cycle the capture rate through the shared presets, persisting it and (since
/// PipeWire negotiates framerate at connect time) restarting capture if running.
pub(in crate::tui) fn cycle_rate(app: &mut App, delta: i32) {
    use super::audio::RATE_PRESETS;
    let cur = RATE_PRESETS
        .iter()
        .position(|&r| r == app.screen.rate_hz)
        .unwrap_or(2) as i32;
    let next = (cur + delta).rem_euclid(RATE_PRESETS.len() as i32) as usize;
    app.screen.rate_hz = RATE_PRESETS[next];
    crate::settings::Settings::update(|s| s.screen_rate_hz = app.screen.rate_hz);

    #[cfg(feature = "screen-capture")]
    if is_screen_mode(app.info.led_mode) && app.screen.is_running() {
        request_stop(app);
        try_reap_run(app);
        // reconcile() on the next tick restarts after REENTRY_COOLDOWN.
    }
    app.status_msg = format!("Screen rate: {} Hz", app.screen.rate_hz);
}

/// Render the live screen color as a filled swatch with its hex value. Only
/// meaningful while running.
pub(in crate::tui) fn render_preview(f: &mut Frame, app: &App, area: Rect) {
    let (r, g, b) = app.screen.color().unwrap_or((0, 0, 0));
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Screen Color  #{r:02X}{g:02X}{b:02X}"));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let swatch_style = Style::default().bg(Color::Rgb(r, g, b));
    let blank = " ".repeat(inner.width as usize);
    let mid = inner.height as usize / 2;
    let mut lines: Vec<Line> = Vec::with_capacity(inner.height as usize);
    for row in 0..inner.height as usize {
        if row == mid {
            let label = format!("rgb({r}, {g}, {b})");
            let w = inner.width as usize;
            let label = if label.len() > w {
                format!("#{r:02X}{g:02X}{b:02X}")
            } else {
                label
            };
            let lpad = w.saturating_sub(label.len()) / 2;
            let rpad = w.saturating_sub(lpad + label.len());
            let text_fg = if r as u16 + g as u16 + b as u16 > 320 {
                Color::Black
            } else {
                Color::White
            };
            lines.push(Line::from(vec![
                Span::styled(" ".repeat(lpad), swatch_style),
                Span::styled(label, swatch_style.fg(text_fg)),
                Span::styled(" ".repeat(rpad), swatch_style),
            ]));
        } else {
            lines.push(Line::from(Span::styled(blank.clone(), swatch_style)));
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
}

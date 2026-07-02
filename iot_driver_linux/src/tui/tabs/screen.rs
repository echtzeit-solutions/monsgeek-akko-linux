//! Screen-reactive state for the Device Info tab.
//!
//! Like the audio visualizer, this is implied by the keyboard's LED mode: when
//! it is ScreenSync (21) the host captures the screen via the XDG ScreenCast
//! portal + PipeWire, averages a region of each frame to one color, applies a
//! calibration transform, and streams it over `SET_SCREEN_COLOR`. Controls
//! (capture **Rate**, **Region**, per-channel gain/gamma + saturation, and a
//! **Test Swatch** for tuning) live in the Device Info tab; a live calibrated
//! color swatch preview appears while it runs.
//!
//! The capture path is gated behind the `screen-capture` Cargo feature (it pulls
//! in `ashpd` + `pipewire`). Without it, selecting ScreenSync shows a hint.

#[cfg(feature = "screen-capture")]
use std::time::{Duration, Instant};

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Points};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::super::App;
use crate::screen_calib::{ColorCalibration, Region};
use crate::settings::Settings;

/// ScreenSync LED mode (host-streamed average screen color).
pub(in crate::tui) const SCREEN_SYNC: u8 = 21;

/// Named screen-region presets cycled by the Region control.
const REGION_PRESETS: &[(&str, Region)] = &[
    (
        "Full",
        Region {
            left: 0.0,
            top: 0.0,
            right: 1.0,
            bottom: 1.0,
        },
    ),
    (
        "No title bar",
        Region {
            left: 0.0,
            top: 0.08,
            right: 1.0,
            bottom: 1.0,
        },
    ),
    (
        "Center 50%",
        Region {
            left: 0.25,
            top: 0.25,
            right: 0.75,
            bottom: 0.75,
        },
    ),
    (
        "Left half",
        Region {
            left: 0.0,
            top: 0.0,
            right: 0.5,
            bottom: 1.0,
        },
    ),
    (
        "Right half",
        Region {
            left: 0.5,
            top: 0.0,
            right: 1.0,
            bottom: 1.0,
        },
    ),
    (
        "Bottom half",
        Region {
            left: 0.0,
            top: 0.5,
            right: 1.0,
            bottom: 1.0,
        },
    ),
];

/// A named calibration-tuning target: label + fixed color (`None` = live average).
type TestSwatch = (&'static str, Option<(u8, u8, u8)>);

/// Fixed calibration-tuning targets. `None` = stream the live screen average.
const TEST_SWATCHES: &[TestSwatch] = &[
    ("Off", None),
    ("White", Some((255, 255, 255))),
    ("Red", Some((255, 0, 0))),
    ("Green", Some((0, 255, 0))),
    ("Blue", Some((0, 0, 255))),
];

/// Clamp/round a calibration scalar after a step (avoids f32 drift in the UI).
fn stepf(v: f32, delta: i32, coarse: bool, min: f32, max: f32) -> f32 {
    let step = if coarse { 0.20 } else { 0.05 };
    let next = (v + delta as f32 * step).clamp(min, max);
    (next * 100.0).round() / 100.0
}

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
    /// Color transform (mirror of the persisted value; pushed live to the run).
    calibration: ColorCalibration,
    /// Capture region (mirror of the persisted value).
    region: Region,
    /// Index into [`TEST_SWATCHES`] — transient tuning aid, not persisted.
    test_swatch: usize,
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
            calibration: ColorCalibration::default(),
            region: Region::default(),
            test_swatch: 0,
            #[cfg(feature = "screen-capture")]
            reentry_after: Instant::now(),
            #[cfg(feature = "screen-capture")]
            run: None,
        }
    }
}

impl ScreenTabState {
    /// Default state seeded with the persisted rate, calibration, and region.
    pub(in crate::tui) fn from_settings(settings: &Settings) -> Self {
        Self {
            rate_hz: settings.screen_rate_hz,
            calibration: settings.screen_calibration,
            region: settings.screen_region,
            ..Self::default()
        }
    }

    /// Calibration mirror (for rendering the control rows).
    pub(in crate::tui) fn calib(&self) -> ColorCalibration {
        self.calibration
    }

    /// Label of the current region (a preset name, or "Custom").
    pub(in crate::tui) fn region_label(&self) -> &'static str {
        REGION_PRESETS
            .iter()
            .find(|(_, r)| *r == self.region)
            .map(|(name, _)| *name)
            .unwrap_or("Custom")
    }

    /// Label of the current test swatch.
    pub(in crate::tui) fn test_swatch_label(&self) -> &'static str {
        TEST_SWATCHES[self.test_swatch].0
    }

    /// Push the current calibration/region/test-swatch to the running capture so
    /// edits take effect live without restarting.
    #[cfg(feature = "screen-capture")]
    fn push_live(&self) {
        if let Some(run) = &self.run {
            run.state.set_calibration(self.calibration);
            run.state.set_region(self.region);
            run.state.set_test_swatch(TEST_SWATCHES[self.test_swatch].1);
        }
    }

    #[cfg(not(feature = "screen-capture"))]
    fn push_live(&self) {}

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

    // The keyboard is already in ScreenSync mode here — set either by the user
    // picking it in the LED mode row (which sent the user's brightness/color via
    // `send_main_led`) or read from the device on connect. So we do NOT resend a
    // placeholder SET_LEDPARAM (that used to overwrite the stored config); we
    // just start streaming the captured color.
    // The fresh capture starts from the persisted calibration/region (loaded in
    // `spawn_for_tui` via `ScreenColorState::from_settings`) with no test swatch;
    // reset the mirror so the displayed swatch matches.
    app.screen.test_swatch = 0;
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

/// Cycle the capture region through [`REGION_PRESETS`]; persist and apply live.
pub(in crate::tui) fn cycle_region(app: &mut App, delta: i32) {
    let cur = REGION_PRESETS
        .iter()
        .position(|(_, r)| *r == app.screen.region)
        .unwrap_or(0) as i32;
    let next = (cur + delta).rem_euclid(REGION_PRESETS.len() as i32) as usize;
    app.screen.region = REGION_PRESETS[next].1;
    Settings::update(|s| s.screen_region = app.screen.region);
    app.screen.push_live();
    app.status_msg = format!("Screen region: {}", REGION_PRESETS[next].0);
}

/// Cycle the calibration test swatch (Off / White / R / G / B). Not persisted.
pub(in crate::tui) fn cycle_test_swatch(app: &mut App, delta: i32) {
    let next = (app.screen.test_swatch as i32 + delta).rem_euclid(TEST_SWATCHES.len() as i32);
    app.screen.test_swatch = next as usize;
    app.screen.push_live();
    app.status_msg = format!("Test swatch: {}", TEST_SWATCHES[app.screen.test_swatch].0);
}

/// Adjust one calibration scalar (`field` selects gain[i]/gamma[i]/saturation);
/// persist and apply live.
pub(in crate::tui) fn adjust_calibration(app: &mut App, field: CalField, delta: i32, coarse: bool) {
    let c = &mut app.screen.calibration;
    match field {
        CalField::GainR => c.gain[0] = stepf(c.gain[0], delta, coarse, 0.0, 2.0),
        CalField::GainG => c.gain[1] = stepf(c.gain[1], delta, coarse, 0.0, 2.0),
        CalField::GainB => c.gain[2] = stepf(c.gain[2], delta, coarse, 0.0, 2.0),
        CalField::GammaR => c.gamma[0] = stepf(c.gamma[0], delta, coarse, 0.3, 3.0),
        CalField::GammaG => c.gamma[1] = stepf(c.gamma[1], delta, coarse, 0.3, 3.0),
        CalField::GammaB => c.gamma[2] = stepf(c.gamma[2], delta, coarse, 0.3, 3.0),
        CalField::Saturation => c.saturation = stepf(c.saturation, delta, coarse, 0.0, 2.0),
    }
    let cal = app.screen.calibration;
    Settings::update(|s| s.screen_calibration = cal);
    app.screen.push_live();
}

/// Which calibration scalar [`adjust_calibration`] targets.
#[derive(Clone, Copy)]
pub(in crate::tui) enum CalField {
    GainR,
    GainG,
    GainB,
    GammaR,
    GammaG,
    GammaB,
    Saturation,
}

/// Render the calibration curves (per-channel input→output) with a collapsed,
/// single-line live color swatch beneath them.
pub(in crate::tui) fn render_preview(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).split(area);
    render_calibration_curves(f, app, rows[0]);
    render_swatch_line(f, app, rows[1]);
}

/// The R/G/B gain+gamma mapping (input 0-255 → output) as one colored line each,
/// over a faint diagonal identity reference. Drawn on a braille [`Canvas`] so the
/// lines render reliably even in a small panel.
fn render_calibration_curves(f: &mut Frame, app: &App, area: Rect) {
    let cal = app.screen.calib();
    const N: u32 = 48;
    let colors = [Color::Red, Color::Green, Color::Blue];

    // Where the current input color (live average, or the active test swatch)
    // lands on each channel's curve: (input, mapped output).
    let raw = TEST_SWATCHES[app.screen.test_swatch]
        .1
        .or_else(|| app.screen.color());
    let markers: Option<[(f64, f64); 3]> = raw.map(|(r, g, b)| {
        let inputs = [r, g, b];
        std::array::from_fn(|ch| (inputs[ch] as f64, cal.channel_map(inputs[ch], ch) as f64))
    });

    let canvas = Canvas::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Calibration  R/G/B in→out"),
        )
        .marker(symbols::Marker::Braille)
        .x_bounds([0.0, 255.0])
        .y_bounds([0.0, 255.0])
        .paint(move |ctx| {
            // Faint identity reference (no-op baseline).
            ctx.draw(&CanvasLine {
                x1: 0.0,
                y1: 0.0,
                x2: 255.0,
                y2: 255.0,
                color: Color::Gray,
            });
            for (ch, &color) in colors.iter().enumerate() {
                let mut prev = (0.0_f64, cal.channel_map(0, ch) as f64);
                for i in 1..=N {
                    let x = (i * 255 / N) as u8;
                    let cur = (x as f64, cal.channel_map(x, ch) as f64);
                    ctx.draw(&CanvasLine {
                        x1: prev.0,
                        y1: prev.1,
                        x2: cur.0,
                        y2: cur.1,
                        color,
                    });
                    prev = cur;
                }
            }

            // Mark where the current color maps on each curve: a channel-colored
            // cross with a white core dot at (input, output).
            if let Some(marks) = markers {
                const ARM: f64 = 7.0;
                for (ch, &(mx, my)) in marks.iter().enumerate() {
                    let color = colors[ch];
                    ctx.draw(&CanvasLine {
                        x1: mx - ARM,
                        y1: my,
                        x2: mx + ARM,
                        y2: my,
                        color,
                    });
                    ctx.draw(&CanvasLine {
                        x1: mx,
                        y1: my - ARM,
                        x2: mx,
                        y2: my + ARM,
                        color,
                    });
                    ctx.draw(&Points {
                        coords: &[(mx, my)],
                        color: Color::White,
                    });
                }
            }
        });
    f.render_widget(canvas, area);
}

/// One-line swatch of the streamed color (calibrated live average or active test
/// swatch) with its hex/rgb label centered on it.
fn render_swatch_line(f: &mut Frame, app: &App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let raw = TEST_SWATCHES[app.screen.test_swatch]
        .1
        .or_else(|| app.screen.color());
    let (r, g, b) = raw
        .map(|c| app.screen.calib().apply(c))
        .unwrap_or((0, 0, 0));

    let bg = Style::default().bg(Color::Rgb(r, g, b));
    let w = area.width as usize;
    let label = format!("#{r:02X}{g:02X}{b:02X}  rgb({r}, {g}, {b})");
    let label = if label.len() > w {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        label
    };
    let lpad = w.saturating_sub(label.len()) / 2;
    let rpad = w.saturating_sub(lpad + label.len());
    let fg = if r as u16 + g as u16 + b as u16 > 320 {
        Color::Black
    } else {
        Color::White
    };
    let line = Line::from(vec![
        Span::styled(" ".repeat(lpad), bg),
        Span::styled(label, bg.fg(fg)),
        Span::styled(" ".repeat(rpad), bg),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

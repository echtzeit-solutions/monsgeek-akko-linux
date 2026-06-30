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

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::super::App;

/// ScreenSync LED mode (host-streamed average screen color).
pub(in crate::tui) const SCREEN_SYNC: u8 = 21;

/// Is this LED mode the host-streamed screen visualizer?
pub(in crate::tui) fn is_screen_mode(led_mode: u8) -> bool {
    led_mode == SCREEN_SYNC
}

#[cfg(feature = "screen-capture")]
struct ScreenRun {
    state: std::sync::Arc<crate::screen_capture::ScreenColorState>,
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(feature = "screen-capture")]
impl Drop for ScreenRun {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        self.running.store(false, Ordering::SeqCst);
        self.state.stop();
    }
}

/// Screen-reactive state, held in [`App`].
pub(in crate::tui) struct ScreenTabState {
    /// Capture rate (Hz) — CPU/USB traffic vs fidelity.
    pub rate_hz: u32,
    pub error: Option<String>,
    /// Whether we've already tried to start for the current mode selection
    /// (avoids retry spam when capture fails or the feature is absent).
    attempted: bool,
    #[cfg(feature = "screen-capture")]
    run: Option<ScreenRun>,
}

impl Default for ScreenTabState {
    fn default() -> Self {
        Self {
            rate_hz: crate::settings::DEFAULT_RATE_HZ,
            error: None,
            attempted: false,
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
        self.run.is_some()
    }

    #[cfg(not(feature = "screen-capture"))]
    pub(in crate::tui) fn is_running(&self) -> bool {
        false
    }

    /// Current averaged screen color, if capturing.
    #[cfg(feature = "screen-capture")]
    fn color(&self) -> Option<(u8, u8, u8)> {
        self.run.as_ref().map(|r| r.state.get_color())
    }

    #[cfg(not(feature = "screen-capture"))]
    fn color(&self) -> Option<(u8, u8, u8)> {
        None
    }
}

/// Reconcile the screen run with the current LED mode: start capturing when
/// ScreenSync is selected, stop when it isn't. Called every tick.
pub(in crate::tui) fn reconcile(app: &mut App) {
    let is_screen = is_screen_mode(app.info.led_mode);

    #[cfg(feature = "screen-capture")]
    {
        if !is_screen {
            if app.screen.run.is_some() {
                app.screen.run = None;
                app.status_msg = "Screen reactive stopped".to_string();
            }
            app.screen.attempted = false;
            return;
        }
        if app.screen.run.is_some() || app.screen.attempted {
            return;
        }
        app.screen.attempted = true;
        start(app);
    }

    #[cfg(not(feature = "screen-capture"))]
    {
        if !is_screen {
            app.screen.attempted = false;
            app.screen.error = None;
        } else if !app.screen.attempted {
            app.screen.attempted = true;
            app.screen.error =
                Some("Screen reactive needs a build with --features screen-capture".to_string());
        }
    }
}

#[cfg(feature = "screen-capture")]
fn start(app: &mut App) {
    use crate::protocol::cmd;

    let Some(keyboard) = app.keyboard.clone() else {
        app.screen.error = Some("No keyboard connected".to_string());
        return;
    };

    // Ensure the device is in ScreenSync mode before we start streaming colors.
    let _ = keyboard.set_led_with_option(cmd::LedMode::ScreenSync.as_u8(), 4, 4, 0, 0, 0, false, 0);

    let (state, running) = crate::screen_capture::spawn_for_tui(keyboard, app.screen.rate_hz);
    app.screen.run = Some(ScreenRun { state, running });
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
    if is_screen_mode(app.info.led_mode) && app.screen.run.is_some() {
        app.screen.run = None; // drop stops capture
        app.screen.attempted = true;
        start(app);
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
    // Fill the panel with the captured color; overlay the rgb readout centered.
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

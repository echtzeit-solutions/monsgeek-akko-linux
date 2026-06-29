//! Audio-reactive state for the Device Info tab.
//!
//! Exposes a capture-source selector, visualizer mode/style, and an enable
//! toggle as Left/Right-cycled settings rows, plus a live level meter panel.
//! Enabling drives the keyboard's native music visualizer (MusicBars /
//! MusicPatterns) over `SET_AUDIO_VIZ` — no flash-wearing per-key streaming.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::super::App;
use crate::audio_reactive::{run_viz_loop, AudioCapture, AudioConfig};
use crate::protocol::cmd::LedMode;
use crate::pulse::{self, SourceEntry};

/// Visualizer mode selectable from the UI.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::tui) enum VizMode {
    Bars,
    Patterns,
}

impl VizMode {
    fn led_mode(self) -> u8 {
        match self {
            VizMode::Bars => LedMode::MusicBars.as_u8(),
            VizMode::Patterns => LedMode::MusicPatterns.as_u8(),
        }
    }
    pub(in crate::tui) fn name(self) -> &'static str {
        match self {
            VizMode::Bars => "Bars",
            VizMode::Patterns => "Patterns",
        }
    }
    /// Highest valid style index for this mode.
    fn max_style(self) -> u8 {
        match self {
            VizMode::Bars => 2,
            VizMode::Patterns => 4,
        }
    }
}

/// A live audio-reactive run: capture threads + the visualizer streaming thread.
struct AudioRun {
    capture: AudioCapture,
    running: Arc<AtomicBool>,
    viz: Option<JoinHandle<()>>,
}

impl Drop for AudioRun {
    fn drop(&mut self) {
        // Stop the viz loop first (it polls `running` every ~20ms), then let the
        // capture's own Drop join its capture/FFT threads.
        self.running.store(false, Ordering::SeqCst);
        if let Some(h) = self.viz.take() {
            let _ = h.join();
        }
        self.capture.stop();
    }
}

/// Audio-reactive state, held in [`App`].
pub(in crate::tui) struct AudioTabState {
    /// Capture sources; `None` until first enumerated.
    pub sources: Option<Vec<SourceEntry>>,
    pub selected: usize,
    pub mode: VizMode,
    pub style: u8,
    pub error: Option<String>,
    run: Option<AudioRun>,
}

impl Default for AudioTabState {
    fn default() -> Self {
        Self {
            sources: None,
            selected: 0,
            mode: VizMode::Bars,
            style: 0,
            error: None,
            run: None,
        }
    }
}

impl AudioTabState {
    pub(in crate::tui) fn is_running(&self) -> bool {
        self.run.is_some()
    }

    /// Description of the currently selected source, if any.
    pub(in crate::tui) fn selected_source_desc(&self) -> Option<&str> {
        self.sources
            .as_ref()?
            .get(self.selected)
            .map(|s| s.description.as_str())
    }
}

/// Enumerate capture sources (lazily, on first Device Info render), preselecting
/// the first monitor source.
pub(in crate::tui) fn ensure_sources_loaded(app: &mut App) {
    if app.audio.sources.is_some() {
        return;
    }
    match pulse::list_sources() {
        Ok(list) => {
            app.audio.selected = list
                .iter()
                .position(|s| s.is_monitor)
                .unwrap_or(0)
                .min(list.len().saturating_sub(1));
            app.audio.sources = Some(list);
            app.audio.error = None;
        }
        Err(e) => {
            app.audio.error = Some(e);
            app.audio.sources = Some(Vec::new());
        }
    }
}

/// Toggle audio-reactive on/off using the current selection.
pub(in crate::tui) fn toggle(app: &mut App) {
    if app.audio.run.is_some() {
        app.audio.run = None; // Drop stops capture + viz threads
        app.status_msg = "Audio reactive stopped".to_string();
    } else {
        start(app);
    }
}

/// Cycle the selected capture source by `delta`, restarting if already running.
pub(in crate::tui) fn cycle_device(app: &mut App, delta: i32) {
    let len = app.audio.sources.as_ref().map_or(0, Vec::len);
    if len == 0 {
        return;
    }
    app.audio.selected = (app.audio.selected as i32 + delta).rem_euclid(len as i32) as usize;
    if app.audio.run.is_some() {
        app.audio.run = None;
        start(app);
    }
}

/// Cycle the visualizer mode (Bars/Patterns), reapplying live if running.
pub(in crate::tui) fn cycle_mode(app: &mut App) {
    app.audio.mode = match app.audio.mode {
        VizMode::Bars => VizMode::Patterns,
        VizMode::Patterns => VizMode::Bars,
    };
    if app.audio.style > app.audio.mode.max_style() {
        app.audio.style = 0;
    }
    reapply_mode(app);
}

/// Adjust the visualizer style by `delta` (wrapping within the mode), reapplying
/// live if running.
pub(in crate::tui) fn cycle_style(app: &mut App, delta: i32) {
    let max = app.audio.mode.max_style() as i32 + 1;
    app.audio.style = (app.audio.style as i32 + delta).rem_euclid(max) as u8;
    reapply_mode(app);
}

fn start(app: &mut App) {
    let Some(keyboard) = app.keyboard.clone() else {
        app.audio.error = Some("No keyboard connected".to_string());
        return;
    };
    let Some(source) = app
        .audio
        .sources
        .as_ref()
        .and_then(|s| s.get(app.audio.selected))
        .cloned()
    else {
        app.audio.error = Some("No capture source selected".to_string());
        return;
    };

    let config = AudioConfig {
        led_mode: app.audio.mode.led_mode(),
        style: app.audio.style,
        sensitivity: 1.0,
        smoothing: 0.3,
        device: Some(source.name.clone()),
    };

    let capture = match AudioCapture::start(config.clone()) {
        Ok(c) => c,
        Err(e) => {
            app.audio.error = Some(e);
            return;
        }
    };

    if let Err(e) = keyboard.set_music_viz_mode(config.led_mode, config.style, 4, 4, false) {
        app.audio.error = Some(format!("Failed to set visualizer mode: {e}"));
    }

    let running = Arc::new(AtomicBool::new(true));
    let viz_state = Arc::clone(&capture.state);
    let viz_running = Arc::clone(&running);
    let viz_kb = Arc::clone(&keyboard);
    let viz_config = config.clone();
    let viz = std::thread::spawn(move || {
        run_viz_loop(&viz_kb, &viz_state, &viz_config, viz_running);
    });

    app.status_msg = format!("Audio reactive: {}", source.label());
    app.audio.error = None;
    app.audio.run = Some(AudioRun {
        capture,
        running,
        viz: Some(viz),
    });
}

/// Apply the current mode/style to the keyboard if a run is active. The viz
/// thread keeps streaming band data regardless; only the on-device render mode
/// changes.
fn reapply_mode(app: &mut App) {
    if app.audio.run.is_none() {
        return;
    }
    if let Some(kb) = app.keyboard.clone() {
        let _ = kb.set_music_viz_mode(app.audio.mode.led_mode(), app.audio.style, 4, 4, false);
    }
}

/// Render the live level meter (8 horizontal bars). Only meaningful while running.
pub(in crate::tui) fn render_meter(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Audio Level Meter");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(run) = app.audio.run.as_ref() else {
        return;
    };
    let bands = run.capture.get_bands();
    let bar_w = inner.width.saturating_sub(4) as usize;
    let lines: Vec<Line> = bands
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let v = v.clamp(0.0, 1.0);
            let filled = ((v * bar_w as f32).round() as usize).min(bar_w);
            let bar = format!("{i} {}{}", "█".repeat(filled), "░".repeat(bar_w - filled));
            let color = if v > 0.85 {
                Color::Red
            } else if v > 0.6 {
                Color::Yellow
            } else {
                Color::Green
            };
            Line::styled(bar, Style::default().fg(color))
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

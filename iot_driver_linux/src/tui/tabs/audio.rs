//! Audio-reactive state for the Device Info tab.
//!
//! Reactive mode is implied by the keyboard's LED mode: when it is MusicBars
//! (22) or MusicPatterns (20), the host captures system audio and streams band
//! levels over `SET_AUDIO_VIZ` so the firmware renders the bars on-device. The
//! only extra controls are the capture **Source** and the visualizer **Style**;
//! they (and the level meter) appear only while a music mode is selected.

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
use crate::pulse::{self, SourceEntry};

const MUSIC_BARS: u8 = 22;
const MUSIC_PATTERNS: u8 = 20;

/// Is this LED mode a host-streamed music visualizer?
pub(in crate::tui) fn is_music_mode(led_mode: u8) -> bool {
    led_mode == MUSIC_BARS || led_mode == MUSIC_PATTERNS
}

/// Highest valid style index for a music mode (Bars: 0-2, Patterns: 0-4).
fn style_max(led_mode: u8) -> u8 {
    match led_mode {
        MUSIC_BARS => 2,
        MUSIC_PATTERNS => 4,
        _ => 0,
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
#[derive(Default)]
pub(in crate::tui) struct AudioTabState {
    /// Capture sources; `None` until first enumerated.
    pub sources: Option<Vec<SourceEntry>>,
    pub selected: usize,
    pub style: u8,
    pub error: Option<String>,
    run: Option<AudioRun>,
    /// LED mode the active run is streaming for (to detect Bars↔Patterns swaps).
    active_mode: Option<u8>,
    /// Last (mode, source) we attempted to start, to avoid retry spam on failure.
    attempted: Option<(u8, usize)>,
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

/// Reconcile the audio run with the current LED mode: start streaming when a
/// music mode is selected, stop when it isn't, and re-apply on Bars↔Patterns
/// swaps. Called every tick.
pub(in crate::tui) fn reconcile(app: &mut App) {
    let mode = app.info.led_mode;

    if !is_music_mode(mode) {
        if app.audio.run.is_some() {
            app.audio.run = None;
            app.status_msg = "Audio reactive stopped".to_string();
        }
        app.audio.active_mode = None;
        app.audio.attempted = None;
        return;
    }

    if app.audio.run.is_some() {
        if app.audio.active_mode != Some(mode) {
            reapply_mode(app, mode);
            app.audio.active_mode = Some(mode);
        }
        return;
    }

    // Want to run but aren't: attempt once per (mode, source) to avoid spam.
    if app.audio.attempted == Some((mode, app.audio.selected)) {
        return;
    }
    app.audio.attempted = Some((mode, app.audio.selected));
    start(app, mode);
}

/// Cycle the selected capture source by `delta`, restarting if already running.
pub(in crate::tui) fn cycle_device(app: &mut App, delta: i32) {
    let len = app.audio.sources.as_ref().map_or(0, Vec::len);
    if len == 0 {
        return;
    }
    app.audio.selected = (app.audio.selected as i32 + delta).rem_euclid(len as i32) as usize;

    let mode = app.info.led_mode;
    if is_music_mode(mode) {
        app.audio.run = None; // drop stops old capture
        app.audio.attempted = Some((mode, app.audio.selected));
        start(app, mode);
    }
}

/// Adjust the visualizer style by `delta` (wrapping within the mode), reapplying
/// live if running.
pub(in crate::tui) fn cycle_style(app: &mut App, delta: i32) {
    let mode = app.info.led_mode;
    let max = style_max(mode) as i32 + 1;
    if max <= 1 {
        return;
    }
    app.audio.style = (app.audio.style as i32 + delta).rem_euclid(max) as u8;
    if app.audio.run.is_some() {
        reapply_mode(app, mode);
    }
}

fn start(app: &mut App, led_mode: u8) {
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
        led_mode,
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

    if let Err(e) = keyboard.set_music_viz_mode(led_mode, app.audio.style, 4, 4, false) {
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
    app.audio.active_mode = Some(led_mode);
    app.audio.run = Some(AudioRun {
        capture,
        running,
        viz: Some(viz),
    });
}

/// Re-send the music mode + current style to the keyboard (used on style change
/// or Bars↔Patterns swap). The viz thread keeps streaming bands.
fn reapply_mode(app: &mut App, led_mode: u8) {
    if let Some(kb) = app.keyboard.clone() {
        let _ = kb.set_music_viz_mode(led_mode, app.audio.style, 4, 4, false);
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

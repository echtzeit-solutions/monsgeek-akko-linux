// Depth Monitor Tab — real-time key depth visualization
//
// All depth-specific rendering and App methods.

use ratatui::{prelude::*, widgets::*};
use std::time::{Duration, Instant};

use monsgeek_keyboard::{led::speed_from_wire, VendorEvent};

use super::super::shared::{DepthViewMode, DEPTH_HISTORY_LEN};
use super::super::App;

// ============================================================================
// Rendering
// ============================================================================

pub(in crate::tui) fn render_depth_monitor(f: &mut Frame, app: &mut App, area: Rect) {
    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    // Status and mode indicator
    let mode_str = match app.depth_view_mode {
        DepthViewMode::BarChart => "Bar Chart",
        DepthViewMode::TimeSeries => "Time Series",
    };
    let status_text = if app.depth_monitoring {
        vec![
            Line::from(vec![
                Span::styled(
                    "MONITORING ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("| View: "),
                Span::styled(mode_str, Style::default().fg(Color::Cyan)),
                Span::raw(" | Active keys: "),
                Span::styled(
                    format!("{}", app.active_keys.len()),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(" | Selected: "),
                Span::styled(
                    format!("{}", app.selected_keys.len()),
                    Style::default().fg(Color::Magenta),
                ),
            ]),
            Line::from("Press 'v' to switch view, Space to select key, 'x' to clear data"),
        ]
    } else {
        vec![
            Line::from(Span::styled(
                "Key depth monitoring is OFF - press 'm' to start",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
        ]
    };
    let status = Paragraph::new(status_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Monitor Status"),
    );
    f.render_widget(status, inner[0]);

    // Main visualization area
    match app.depth_view_mode {
        DepthViewMode::BarChart => render_depth_bar_chart(f, app, inner[1]),
        DepthViewMode::TimeSeries => render_depth_time_series(f, app, inner[1]),
    }

    // Help bar
    let help_text = if app.depth_monitoring {
        match app.depth_view_mode {
            DepthViewMode::BarChart => "m:Stop  v:TimeSeries  ↑↓←→:Navigate  Space:Select  x:Clear",
            DepthViewMode::TimeSeries => "m:Stop  v:BarChart  Space:Deselect  x:Clear",
        }
    } else {
        "m:Start monitoring  v:Switch view"
    };
    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(help, inner[2]);
}

fn render_depth_bar_chart(f: &mut Frame, app: &mut App, area: Rect) {
    // Use max observed depth for consistent scaling (minimum 0.1mm)
    let max_depth = app.max_observed_depth.max(0.1);

    // Show all keys with non-zero depth as a single row of bars
    // Use raw depth values (scaled to u64 for display) - BarChart handles scaling via .max()
    let mut bar_data: Vec<(String, u64)> = Vec::new();

    for (i, &depth) in app.key_depths.iter().enumerate() {
        if depth > 0.01 || app.active_keys.contains(&i) {
            // Convert mm to 0.01mm units for integer display
            let depth_raw = (depth * 100.0) as u64;
            let label = get_key_label(app, i);
            bar_data.push((label, depth_raw));
        }
    }

    // If no active keys, show placeholder
    if bar_data.is_empty() {
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "No keys pressed",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from("Press keys to see their depth"),
        ];
        let para = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Key Depths (max: {max_depth:.1}mm)")),
        );
        f.render_widget(para, area);
        return;
    }

    // Convert to references for BarChart
    let bar_refs: Vec<(&str, u64)> = bar_data.iter().map(|(s, v)| (s.as_str(), *v)).collect();

    // Max in same units as data (0.01mm)
    let max_raw = (max_depth * 100.0) as u64;

    let chart = BarChart::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Key Depths (max: {max_depth:.2}mm)")),
        )
        .max(max_raw)
        .bar_width(3)
        .bar_gap(1)
        .bar_style(Style::default().fg(Color::Cyan))
        .value_style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::DIM),
        )
        .data(&bar_refs);

    f.render_widget(chart, area);
}

fn render_depth_time_series(f: &mut Frame, app: &mut App, area: Rect) {
    // Find all keys with history data (any non-empty history)
    let mut active_keys: Vec<usize> = app
        .depth_history
        .iter()
        .enumerate()
        .filter(|(_, h)| !h.is_empty())
        .map(|(i, _)| i)
        .collect();
    active_keys.sort();

    // Limit to first 8 keys for readability
    active_keys.truncate(8);

    if active_keys.is_empty() {
        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "No key activity recorded yet",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from("Press keys while monitoring to see their depth over time"),
        ];
        let para = Paragraph::new(text)
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title("Time Series"));
        f.render_widget(para, area);
        return;
    }

    // Colors for different keys
    let colors = [
        Color::Cyan,
        Color::Yellow,
        Color::Green,
        Color::Magenta,
        Color::Red,
        Color::Blue,
        Color::LightCyan,
        Color::LightYellow,
    ];

    // Calculate time bounds from all histories (timestamps are in seconds since start)
    let time_window = 5.0; // Show 5 seconds of history
    let mut time_min = f64::MAX;
    let mut time_max = f64::MIN;
    for &key_idx in &active_keys {
        if let Some(history) = app.depth_history.get(key_idx) {
            for &(t, _) in history.iter() {
                if t < time_min {
                    time_min = t;
                }
                if t > time_max {
                    time_max = t;
                }
            }
        }
    }

    // If no valid timestamps, show empty
    if time_min == f64::MAX || time_max == f64::MIN {
        time_min = 0.0;
        time_max = time_window;
    }

    // Show a rolling window of the last N seconds
    let x_max = time_max;
    let x_min = (time_max - time_window).max(time_min);

    // Build datasets for Chart widget
    let mut datasets: Vec<Dataset> = Vec::new();
    let mut all_data: Vec<Vec<(f64, f64)>> = Vec::new();

    for (color_idx, &key_idx) in active_keys.iter().enumerate() {
        if key_idx < app.depth_history.len() {
            let history = &app.depth_history[key_idx];
            // Use actual timestamps as X-axis, filter to visible window
            let data: Vec<(f64, f64)> = history
                .iter()
                .filter(|(t, _)| *t >= x_min)
                .map(|&(t, depth)| (t, depth as f64))
                .collect();
            all_data.push(data);

            let color = colors[color_idx % colors.len()];
            let label = get_key_label(app, key_idx);
            datasets.push(
                Dataset::default()
                    .name(label)
                    .marker(symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(color)),
            );
        }
    }

    // Set data references
    let datasets: Vec<Dataset> = datasets
        .into_iter()
        .zip(all_data.iter())
        .map(|(ds, data)| ds.data(data))
        .collect();

    // Build legend string
    let legend: String = active_keys
        .iter()
        .enumerate()
        .map(|(i, &k)| {
            let color_char = match i {
                0 => "C",
                1 => "Y",
                2 => "G",
                3 => "M",
                4 => "R",
                5 => "B",
                6 => "c",
                7 => "y",
                _ => "?",
            };
            format!("[{}]K{}", color_char, get_key_label(app, k))
        })
        .collect::<Vec<_>>()
        .join(" ");

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Time Series: {legend}")),
        )
        .x_axis(
            Axis::default()
                .title("Time (s)")
                .style(Style::default().fg(Color::Gray))
                .bounds([x_min, x_max])
                .labels(vec![
                    Span::raw(format!("{:.1}", x_min)),
                    Span::raw(format!("{:.1}", (x_min + x_max) / 2.0)),
                    Span::raw(format!("{:.1}", x_max)),
                ]),
        )
        .y_axis(
            Axis::default()
                .title("mm")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, 4.5])
                .labels(vec![
                    Span::raw("0"),
                    Span::raw("1"),
                    Span::raw("2"),
                    Span::raw("3"),
                    Span::raw("4"),
                ]),
        );

    f.render_widget(chart, area);
}

/// Get key label for display - use device profile matrix key names
pub(in crate::tui) fn get_key_label(app: &App, index: usize) -> String {
    app.matrix_key_names
        .get(index)
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_default()
}

// ============================================================================
// App methods
// ============================================================================

impl App {
    /// Handle stale key cleanup on tick (primary release detection is via depth < 0.05 threshold)
    pub(in crate::tui) fn read_input_reports(&mut self) {
        if !self.depth_monitoring {
            return;
        }

        // Depth events are now handled via handle_vendor_notification from the transport layer.
        // This function handles stale key cleanup and history updates on tick.

        let now = Instant::now();
        // Long timeout as fallback - primary release detection is via depth < 0.05 threshold
        // This only catches truly stale keys (e.g., missed release reports)
        let stale_timeout = Duration::from_secs(2);

        // Reset keys that haven't been updated recently (considered released)
        let stale_keys: Vec<usize> = self
            .active_keys
            .iter()
            .filter(|&&k| {
                k < self.depth_last_update.len()
                    && now.duration_since(self.depth_last_update[k]) > stale_timeout
            })
            .copied()
            .collect();
        for key_idx in stale_keys {
            if key_idx < self.key_depths.len() {
                self.key_depths[key_idx] = 0.0;
            }
            if key_idx < self.depth_history.len() {
                self.depth_history[key_idx].clear();
            }
            self.active_keys.remove(&key_idx);
        }
        // Note: depth_history is now updated in handle_depth_event with real timestamps
    }

    /// Handle a vendor notification and update app state
    pub(in crate::tui) fn handle_vendor_notification(
        &mut self,
        timestamp: f64,
        event: VendorEvent,
    ) {
        match event {
            VendorEvent::Wake => {
                self.status_msg = "Keyboard wake".to_string();
            }
            VendorEvent::ProfileChange { profile } => {
                self.info.profile = profile;
                self.status_msg = format!("Profile {} (via Fn key)", profile + 1);
            }
            VendorEvent::LedEffectMode { effect_id } => {
                self.info.led_mode = effect_id;
                self.status_msg = format!(
                    "LED mode: {} ({})",
                    effect_id,
                    crate::cmd::led_mode_name(effect_id)
                );
            }
            VendorEvent::LedEffectSpeed { speed } => {
                self.info.led_speed = speed_from_wire(speed);
                self.status_msg = format!("LED speed: {}/4", speed);
            }
            VendorEvent::BrightnessLevel { level } => {
                self.info.led_brightness = level;
                self.status_msg = format!("Brightness: {}/4", level);
            }
            VendorEvent::LedColor { color } => {
                // Map color index to RGB (7 preset colors + custom)
                let (r, g, b) = match color {
                    0 => (255, 0, 0),     // Red
                    1 => (255, 128, 0),   // Orange
                    2 => (255, 255, 0),   // Yellow
                    3 => (0, 255, 0),     // Green
                    4 => (0, 255, 255),   // Cyan
                    5 => (0, 0, 255),     // Blue
                    6 => (128, 0, 255),   // Purple
                    7 => (255, 255, 255), // White (rainbow/dazzle)
                    _ => (255, 255, 255), // Default white
                };
                self.info.led_r = r;
                self.info.led_g = g;
                self.info.led_b = b;
                // Color index 7 typically means dazzle/rainbow mode
                self.info.led_dazzle = color == 7;
                self.status_msg = format!("LED color: #{:02X}{:02X}{:02X}", r, g, b);
            }
            VendorEvent::WinLockToggle { locked } => {
                self.status_msg =
                    format!("Win key: {}", if locked { "LOCKED" } else { "unlocked" });
            }
            VendorEvent::WasdSwapToggle { swapped } => {
                self.info.wasd_swap = swapped;
                self.status_msg = format!(
                    "WASD/Arrows: {}",
                    if swapped { "SWAPPED" } else { "normal" }
                );
            }
            VendorEvent::BacklightToggle => {
                self.status_msg = "Backlight toggled".to_string();
            }
            VendorEvent::DialModeToggle => {
                self.status_msg = "Dial mode toggled".to_string();
            }
            VendorEvent::FnLayerToggle { layer } => {
                self.status_msg = format!("Fn layer: {}", layer);
            }
            VendorEvent::SettingsAck { started } => {
                // Settings ACK is low-level, only show in debug
                if started {
                    tracing::debug!("Settings change started");
                } else {
                    tracing::debug!("Settings change complete");
                }
            }
            VendorEvent::MagnetismStart | VendorEvent::MagnetismStop => {
                // Handled separately by depth monitoring
            }
            VendorEvent::KeyDepth {
                key_index,
                depth_raw,
            } => {
                self.handle_depth_event(key_index, depth_raw, timestamp);
            }
            VendorEvent::BatteryStatus {
                level,
                charging,
                online,
            } => {
                self.battery = Some(crate::hid::BatteryInfo {
                    level,
                    charging,
                    online,
                    idle: false,
                });
            }
            VendorEvent::UnknownKbFunc { category, action } => {
                self.status_msg = format!("KB func: cat={} action={}", category, action);
            }
            VendorEvent::MouseReport {
                buttons,
                x,
                y,
                wheel,
            } => {
                // Mouse reports from keyboard's built-in mouse function
                tracing::debug!(
                    "Mouse: buttons={:#04x} x={} y={} wheel={}",
                    buttons,
                    x,
                    y,
                    wheel
                );
            }
            VendorEvent::Unknown(data) => {
                tracing::debug!("Unknown notification: {:02X?}", data);
            }
        }
    }

    /// Handle a depth event from the keyboard (coalesced from event loop)
    pub(in crate::tui) fn handle_depth_event(
        &mut self,
        key_index: u8,
        depth_raw: u16,
        timestamp: f64,
    ) {
        let precision = self.precision.factor() as f32;
        let depth_mm = depth_raw as f32 / precision;
        let key_index = key_index as usize;

        if key_index < self.key_depths.len() {
            // Update current depth (for bar chart)
            self.key_depths[key_index] = depth_mm;

            // Feed depth to modal if open and matching filter
            if let Some(ref mut modal) = self.trigger_edit_modal {
                let should_sample = match modal.depth_filter {
                    Some(filter_key) => filter_key == key_index,
                    None => true, // No filter = sample all keys (use max)
                };
                if should_sample {
                    modal.push_depth(depth_mm);
                }
            }

            // Update timestamp for this key (for stale detection)
            if key_index < self.depth_last_update.len() {
                self.depth_last_update[key_index] = Instant::now();
            }

            // Track max observed depth for bar chart scaling
            if depth_mm > self.max_observed_depth {
                self.max_observed_depth = depth_mm;
            }

            // Mark key as active when pressed, remove when fully released
            if depth_mm > 0.1 {
                self.active_keys.insert(key_index);
                // Push to timestamped history (for time series)
                if key_index < self.depth_history.len() {
                    let history = &mut self.depth_history[key_index];
                    if history.len() >= DEPTH_HISTORY_LEN {
                        history.pop_front();
                    }
                    history.push_back((timestamp, depth_mm));
                }
            } else if depth_mm < 0.05 {
                // Only remove when depth is very close to rest position
                // This handles the "key released" report from keyboard
                self.active_keys.remove(&key_index);
                // Clear time series history for this key
                if key_index < self.depth_history.len() {
                    self.depth_history[key_index].clear();
                }
            }
            // Note: depths between 0.05-0.1 keep current state (hysteresis)
        }
    }

    /// Toggle view mode for depth tab
    pub(in crate::tui) fn toggle_depth_view(&mut self) {
        self.depth_view_mode = match self.depth_view_mode {
            DepthViewMode::BarChart => DepthViewMode::TimeSeries,
            DepthViewMode::TimeSeries => DepthViewMode::BarChart,
        };
        self.status_msg = format!("Depth view: {:?}", self.depth_view_mode);
    }

    /// Toggle selection of a key for time series view
    pub(in crate::tui) fn toggle_key_selection(&mut self, key_index: usize) {
        if self.selected_keys.contains(&key_index) {
            self.selected_keys.remove(&key_index);
        } else if self.selected_keys.len() < 8 {
            // Limit to 8 selected keys for readability
            self.selected_keys.insert(key_index);
        }
    }

    /// Clear depth history and active keys
    pub(in crate::tui) fn clear_depth_data(&mut self) {
        for history in &mut self.depth_history {
            history.clear();
        }
        self.active_keys.clear();
        for depth in &mut self.key_depths {
            *depth = 0.0;
        }
        self.status_msg = "Depth data cleared".to_string();
    }
}

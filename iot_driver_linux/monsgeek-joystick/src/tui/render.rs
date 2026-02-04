//! TUI rendering logic

use crate::config::{AxisConfig, AxisMappingMode};
use crate::joystick::{AXIS_MAX, AXIS_MIN};
use crate::tui::app::{App, AppMode, JoystickStatus, KeyboardStatus, SelectedElement};
use crate::tui::keyboard_layout::render_keyboard_layout;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use ratatui::Frame;

/// Render the entire application UI
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab bar
            Constraint::Min(10),   // Main content
            Constraint::Length(3), // Status bar
        ])
        .split(frame.area());

    render_tabs(frame, app, chunks[0]);
    render_main_content(frame, app, chunks[1]);
    render_status_bar(frame, app, chunks[2]);

    if app.show_help {
        render_help_overlay(frame, app);
    }
}

/// Render the tab bar
fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles = vec!["Axes", "Configure", "Calibrate"];
    let selected = match app.mode {
        AppMode::Live => 0,
        AppMode::Configure => 1,
        AppMode::Calibrate => 2,
    };

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" MonsGeek Joystick Mapper "),
        )
        .select(selected)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(tabs, area);
}

/// Render main content based on mode
fn render_main_content(frame: &mut Frame, app: &App, area: Rect) {
    match app.mode {
        AppMode::Live => render_live_view(frame, app, area),
        AppMode::Configure => render_configure_view(frame, app, area),
        AppMode::Calibrate => render_calibrate_view(frame, app, area),
    }
}

/// Render live view with axis gauges and key depths
fn render_live_view(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Left: Axis gauges
    render_axis_gauges(frame, app, chunks[0]);

    // Right: Keyboard layout with depths
    let kb_block = Block::default().borders(Borders::ALL).title(" Key Depths ");
    let inner = kb_block.inner(chunks[1]);
    frame.render_widget(kb_block, chunks[1]);
    render_keyboard_layout(frame, app, inner);
}

/// Render axis gauges
fn render_axis_gauges(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Live Axis View ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let axis_count = app.config.axes.len();
    if axis_count == 0 {
        let msg = Paragraph::new("No axes configured");
        frame.render_widget(msg, inner);
        return;
    }

    let constraints: Vec<Constraint> = (0..axis_count).map(|_| Constraint::Length(2)).collect();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    for (i, axis_config) in app.config.axes.iter().enumerate() {
        if i >= rows.len() {
            break;
        }
        render_axis_row(frame, app, axis_config, rows[i]);
    }
}

/// Render a single axis row (label + gauge + value)
fn render_axis_row(frame: &mut Frame, app: &App, axis: &AxisConfig, area: Rect) {
    let value = app.mapper.get_axis_value(axis.id);
    let keys_str = match &axis.mapping {
        AxisMappingMode::TwoKey {
            positive_key,
            negative_key,
        } => format!("{}/{}", positive_key.position, negative_key.position),
        AxisMappingMode::SingleKey { key, .. } => key.position.to_string(),
    };

    let label = format!("{} ({}) ", axis.id.display_name(), keys_str);

    // Normalize value to 0.0-1.0 (unused since we use bipolar gauge)
    let _normalized = ((value - AXIS_MIN) as f64) / ((AXIS_MAX - AXIS_MIN) as f64);

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(12), // Label
            Constraint::Min(20),    // Gauge
            Constraint::Length(8),  // Value
        ])
        .split(area);

    // Label
    let style = if axis.enabled {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let label_widget = Paragraph::new(label).style(style);
    frame.render_widget(label_widget, chunks[0]);

    // Gauge (centered for bipolar display)
    let gauge_color = if value > 0 {
        Color::Green
    } else if value < 0 {
        Color::Red
    } else {
        Color::DarkGray
    };

    // For bipolar axis, show gauge from center
    // We'll use a custom rendering approach
    render_bipolar_gauge(frame, value, chunks[1], gauge_color);

    // Value
    let value_str = format!("{:>6}", value);
    let value_widget = Paragraph::new(value_str).style(style);
    frame.render_widget(value_widget, chunks[2]);
}

/// Render a bipolar gauge (center = 0, left = negative, right = positive)
fn render_bipolar_gauge(frame: &mut Frame, value: i32, area: Rect, color: Color) {
    let width = area.width as usize;
    if width < 3 {
        return;
    }

    let center = width / 2;
    // Calculate fill position
    let normalized = (value as f64 / AXIS_MAX as f64).clamp(-1.0, 1.0);
    let fill_width = ((normalized.abs() * center as f64) as usize).min(center);

    let mut chars: Vec<Span> = Vec::with_capacity(width);

    // Build the gauge string
    for i in 0..width {
        if i == center {
            chars.push(Span::styled("|", Style::default().fg(Color::White)));
        } else if (value >= 0 && i > center && i <= center + fill_width)
            || (value < 0 && i < center && i >= center - fill_width)
        {
            chars.push(Span::styled("=", Style::default().fg(color)));
        } else {
            chars.push(Span::styled("-", Style::default().fg(Color::DarkGray)));
        }
    }

    let line = Line::from(chars);
    let widget = Paragraph::new(line);
    frame.render_widget(widget, area);
}

/// Render configuration view
fn render_configure_view(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // Left: Axis list
    render_axis_list(frame, app, chunks[0]);

    // Right: Selected axis details or keyboard
    if app.awaiting_key_press {
        let kb_block = Block::default()
            .borders(Borders::ALL)
            .title(" Press a key to map ");
        let inner = kb_block.inner(chunks[1]);
        frame.render_widget(kb_block, chunks[1]);
        render_keyboard_layout(frame, app, inner);
    } else {
        render_axis_details(frame, app, chunks[1]);
    }
}

/// Render axis list for selection
fn render_axis_list(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(" Axes ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let selected_idx = match app.selected {
        SelectedElement::AxisList(i) => Some(i),
        SelectedElement::AxisProperty(i, _) => Some(i),
        _ => None,
    };

    let lines: Vec<Line> = app
        .config
        .axes
        .iter()
        .enumerate()
        .map(|(i, axis)| {
            let marker = if Some(i) == selected_idx { "> " } else { "  " };
            let enabled = if axis.enabled { "[x]" } else { "[ ]" };
            let text = format!("{}{} {}", marker, enabled, axis.id.display_name());
            let style = if Some(i) == selected_idx {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if axis.enabled {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Line::from(Span::styled(text, style))
        })
        .collect();

    let widget = Paragraph::new(lines);
    frame.render_widget(widget, inner);
}

/// Render axis details panel
fn render_axis_details(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Configuration ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let axis_idx = match app.selected {
        SelectedElement::AxisList(i) => i,
        SelectedElement::AxisProperty(i, _) => i,
        _ => return,
    };

    let axis = match app.config.axes.get(axis_idx) {
        Some(a) => a,
        None => return,
    };

    let mut lines = Vec::new();

    // Mode
    let mode_str = match &axis.mapping {
        AxisMappingMode::TwoKey { .. } => "Two Keys",
        AxisMappingMode::SingleKey { invert, .. } => {
            if *invert {
                "Single Key (inverted)"
            } else {
                "Single Key"
            }
        }
    };
    lines.push(Line::from(format!("Mode: {}", mode_str)));

    // Keys
    match &axis.mapping {
        AxisMappingMode::TwoKey {
            positive_key,
            negative_key,
        } => {
            lines.push(Line::from(format!(
                "Positive: {} (idx:{})",
                positive_key.position, positive_key.index
            )));
            lines.push(Line::from(format!(
                "Negative: {} (idx:{})",
                negative_key.position, negative_key.index
            )));
        }
        AxisMappingMode::SingleKey { key, .. } => {
            lines.push(Line::from(format!(
                "Key: {} (idx:{})",
                key.position, key.index
            )));
        }
    }

    // Calibration
    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "Deadzone: {:.0}%",
        axis.calibration.deadzone_percent
    )));
    lines.push(Line::from(format!(
        "Curve: {:.1}",
        axis.calibration.curve_exponent
    )));

    let widget = Paragraph::new(lines);
    frame.render_widget(widget, inner);
}

/// Render calibration view
fn render_calibrate_view(frame: &mut Frame, _app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Calibration ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = vec![
        Line::from("Calibration Mode"),
        Line::from(""),
        Line::from("1. Press and hold keys to see their max travel"),
        Line::from("2. Adjust max_travel_mm in config"),
        Line::from(""),
        Line::from("Current readings:"),
    ];

    let widget = Paragraph::new(lines);
    frame.render_widget(widget, inner);
}

/// Render status bar
fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let kb_status = match app.keyboard_status {
        KeyboardStatus::Disconnected => {
            Span::styled("KB: Disconnected", Style::default().fg(Color::Red))
        }
        KeyboardStatus::Connecting => {
            Span::styled("KB: Connecting...", Style::default().fg(Color::Yellow))
        }
        KeyboardStatus::Connected => {
            Span::styled("KB: Connected", Style::default().fg(Color::Green))
        }
        KeyboardStatus::Error => Span::styled("KB: Error", Style::default().fg(Color::Red)),
    };

    let js_status = match app.joystick_status {
        JoystickStatus::NotCreated => {
            Span::styled("Joy: Not Created", Style::default().fg(Color::Red))
        }
        JoystickStatus::Active => Span::styled("Joy: Active", Style::default().fg(Color::Green)),
        JoystickStatus::Error => Span::styled("Joy: Error", Style::default().fg(Color::Red)),
    };

    let dirty_marker = if app.config_dirty { " [*]" } else { "" };

    let help_text = "? help | q quit | s save";

    let status_line = Line::from(vec![
        Span::raw("["),
        kb_status,
        Span::raw("] ["),
        js_status,
        Span::raw("]"),
        Span::raw(dirty_marker),
        Span::raw(" | "),
        Span::styled(help_text, Style::default().fg(Color::DarkGray)),
    ]);

    let block = Block::default().borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let widget = Paragraph::new(status_line);
    frame.render_widget(widget, inner);
}

/// Render help overlay
fn render_help_overlay(frame: &mut Frame, _app: &App) {
    let area = centered_rect(60, 50, frame.area());

    let help_text = vec![
        Line::from("Keyboard Shortcuts"),
        Line::from(""),
        Line::from("Tab / 1-3    Switch modes"),
        Line::from("Arrow keys   Navigate"),
        Line::from("Enter        Select/Edit"),
        Line::from("Escape       Go back"),
        Line::from("Space        Toggle enabled"),
        Line::from("+/-          Adjust value"),
        Line::from("s            Save config"),
        Line::from("q            Quit"),
        Line::from("?            Toggle help"),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help ")
        .style(Style::default().bg(Color::Black));

    let widget = Paragraph::new(help_text).block(block);
    frame.render_widget(widget, area);
}

/// Helper to create a centered rectangle
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

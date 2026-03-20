// Help system - Self-documenting keybindings

use ratatui::{prelude::*, widgets::*};

/// Context in which a keybind is active
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum KeyContext {
    Global,   // Available everywhere
    Info,     // Device Info tab (0)
    Depth,    // Key Depth tab (1)
    Triggers, // Trigger Settings tab (2)
    Remaps,   // Remaps tab (3)
    #[cfg(feature = "notify")]
    Notify, // Notify tab (4)
}

/// A single keybinding definition
pub(crate) struct Keybind {
    pub keys: &'static str,
    pub description: &'static str,
    pub context: KeyContext,
}

/// All TUI keybindings - single source of truth
pub(crate) const TUI_KEYBINDS: &[Keybind] = &[
    // Global keybindings
    Keybind {
        keys: "q / Esc",
        description: "Quit application",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "? / F1",
        description: "Toggle this help",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "Tab",
        description: "Next tab",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "Shift+Tab",
        description: "Previous tab",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "↑ / k",
        description: "Navigate up",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "↓ / j",
        description: "Navigate down",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "← / h",
        description: "Navigate left / decrease",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "→ / l",
        description: "Navigate right / increase",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "r",
        description: "Refresh device info",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "c",
        description: "Connect to device",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "d",
        description: "Device picker",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "m",
        description: "Toggle depth monitoring",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "Ctrl+1-4",
        description: "Switch profile 1-4",
        context: KeyContext::Global,
    },
    Keybind {
        keys: "PgUp/PgDn",
        description: "Fast scroll (15 items)",
        context: KeyContext::Global,
    },
    // Info tab
    Keybind {
        keys: "p",
        description: "Apply per-key LED color",
        context: KeyContext::Info,
    },
    Keybind {
        keys: "Shift+←/→",
        description: "Coarse adjust (±10 for RGB)",
        context: KeyContext::Info,
    },
    // Depth tab
    Keybind {
        keys: "v",
        description: "Toggle visualization mode",
        context: KeyContext::Depth,
    },
    Keybind {
        keys: "x",
        description: "Clear depth data",
        context: KeyContext::Depth,
    },
    Keybind {
        keys: "Space",
        description: "Pause/resume monitoring",
        context: KeyContext::Depth,
    },
    // Triggers tab
    Keybind {
        keys: "v",
        description: "Toggle list/layout view",
        context: KeyContext::Triggers,
    },
    Keybind {
        keys: "Enter / e",
        description: "Edit selected key",
        context: KeyContext::Triggers,
    },
    Keybind {
        keys: "g",
        description: "Edit global (all keys)",
        context: KeyContext::Triggers,
    },
    Keybind {
        keys: "n / N",
        description: "Normal mode (key/all)",
        context: KeyContext::Triggers,
    },
    Keybind {
        keys: "t / T",
        description: "RT mode (key/all)",
        context: KeyContext::Triggers,
    },
    Keybind {
        keys: "d / D",
        description: "DKS mode (key/all)",
        context: KeyContext::Triggers,
    },
    Keybind {
        keys: "s / S",
        description: "SnapTap mode (key/all)",
        context: KeyContext::Triggers,
    },
    // Remaps tab
    Keybind {
        keys: "e",
        description: "Edit remap target",
        context: KeyContext::Remaps,
    },
    Keybind {
        keys: "d",
        description: "Reset key to default",
        context: KeyContext::Remaps,
    },
    Keybind {
        keys: "m",
        description: "Open macro editor",
        context: KeyContext::Remaps,
    },
    Keybind {
        keys: "f",
        description: "Toggle layer filter",
        context: KeyContext::Remaps,
    },
    // Notify tab
    #[cfg(feature = "notify")]
    Keybind {
        keys: "n",
        description: "Toggle daemon start/stop",
        context: KeyContext::Notify,
    },
    #[cfg(feature = "notify")]
    Keybind {
        keys: "p",
        description: "Toggle hardware preview",
        context: KeyContext::Notify,
    },
    #[cfg(feature = "notify")]
    Keybind {
        keys: "s",
        description: "Save effects.toml",
        context: KeyContext::Notify,
    },
    #[cfg(feature = "notify")]
    Keybind {
        keys: "Enter",
        description: "Edit keyframes / confirm",
        context: KeyContext::Notify,
    },
    #[cfg(feature = "notify")]
    Keybind {
        keys: "a / x",
        description: "Add / delete keyframe",
        context: KeyContext::Notify,
    },
    #[cfg(feature = "notify")]
    Keybind {
        keys: "Tab",
        description: "Switch focus / edit variables",
        context: KeyContext::Notify,
    },
];

/// Physical keyboard shortcuts from the manual
pub(crate) const KEYBOARD_SHORTCUTS: &[(&str, &str)] = &[
    // Profile switching
    ("Fn+F9", "Profile 1"),
    ("Fn+F10", "Profile 2"),
    ("Fn+F11", "Profile 3"),
    ("Fn+F12", "Profile 4"),
    // LED controls
    ("Fn+\\", "Cycle 7 colors + RGB"),
    ("Fn+↑", "Brightness up"),
    ("Fn+↓", "Brightness down"),
    ("Fn+←", "LED speed down"),
    ("Fn+→", "LED speed up"),
    ("Fn+=", "LED settings"),
    ("Fn+L", "LED mode cycle"),
    ("Fn+Home", "Effect 1-5"),
    ("Fn+PgUp", "Effect 6-10"),
    ("Fn+End", "Effect 11-15"),
    ("Fn+PgDn", "Effect 16-20"),
    // Connection modes
    ("Fn+E/R/T", "Bluetooth 1/2/3 (long=pair)"),
    ("Fn+Y", "2.4GHz mode (long=pair)"),
    // Utility
    ("Fn+Space", "Battery check"),
    ("Fn+W", "WASD/Arrow swap"),
    ("Fn+L_Win", "Win key lock"),
    ("Fn+I", "Insert"),
    ("Fn+P", "Print Screen"),
    ("Fn+C", "Calculator"),
    // Media (Windows)
    ("Fn+F1", "File Explorer"),
    ("Fn+F2", "Mail"),
    ("Fn+F3", "Browser"),
    ("Fn+F4", "Lock PC"),
    ("Fn+F5", "Display off"),
    ("Fn+F6/F8", "Play/Pause"),
    ("Fn+F7", "Volume down"),
    ("Fn+M", "Mute"),
    ("Fn+<", "Volume down"),
    ("Fn+>", "Volume up"),
];

/// Render help popup with all keybindings
pub(crate) fn render_help_popup(f: &mut Frame, area: Rect) {
    // Calculate popup size (80% width, 80% height)
    let popup_width = (area.width as f32 * 0.85) as u16;
    let popup_height = (area.height as f32 * 0.85) as u16;
    let popup_x = (area.width - popup_width) / 2;
    let popup_y = (area.height - popup_height) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the area behind the popup
    f.render_widget(Clear, popup_area);

    // Split into two columns: TUI shortcuts and Keyboard shortcuts
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(popup_area);

    // Left column: TUI Keybindings
    let mut tui_lines: Vec<Line> = vec![Line::from(Span::styled(
        "── Global ──",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))];

    let mut current_context = KeyContext::Global;
    for kb in TUI_KEYBINDS {
        if kb.context != current_context {
            current_context = kb.context;
            let section_name = match current_context {
                KeyContext::Global => "Global",
                KeyContext::Info => "Info Tab",
                KeyContext::Depth => "Depth Tab",
                KeyContext::Triggers => "Triggers Tab",
                KeyContext::Remaps => "Remaps Tab",
                #[cfg(feature = "notify")]
                KeyContext::Notify => "Notify Tab",
            };
            tui_lines.push(Line::from(""));
            tui_lines.push(Line::from(Span::styled(
                format!("── {section_name} ──"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        tui_lines.push(Line::from(vec![
            Span::styled(format!("{:14}", kb.keys), Style::default().fg(Color::Cyan)),
            Span::raw(" "),
            Span::raw(kb.description),
        ]));
    }

    let tui_help = Paragraph::new(tui_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" TUI Shortcuts [? to close] ")
                .title_style(
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(tui_help, columns[0]);

    // Right column: Physical Keyboard Shortcuts
    let mut kb_lines: Vec<Line> = vec![Line::from(Span::styled(
        "── Profiles ──",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))];

    let sections = [
        (0, 4, "Profiles"),
        (4, 12, "LED Controls"),
        (12, 14, "Connection"),
        (14, 19, "Utility"),
        (19, 29, "Media (Win)"),
    ];

    for (idx, (start, end, name)) in sections.into_iter().enumerate() {
        if idx > 0 {
            kb_lines.push(Line::from(""));
            kb_lines.push(Line::from(Span::styled(
                format!("── {name} ──"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        for (key, desc) in &KEYBOARD_SHORTCUTS[start..end] {
            kb_lines.push(Line::from(vec![
                Span::styled(format!("{key:14}"), Style::default().fg(Color::Magenta)),
                Span::raw(" "),
                Span::raw(*desc),
            ]));
        }
    }

    let kb_help = Paragraph::new(kb_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Physical Keyboard (Fn+key) ")
                .title_style(
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(kb_help, columns[1]);
}

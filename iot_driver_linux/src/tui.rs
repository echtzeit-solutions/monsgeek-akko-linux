// MonsGeek M1 V5 HE TUI Application
// Real-time monitoring and settings configuration

use std::io::{self, stdout};
use std::time::{Duration, Instant};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::*,
};

// Use shared library
use iot_driver::{cmd, MonsGeekDevice, DeviceInfo, TriggerSettings, magnetism};

/// Application state
struct App {
    device: Option<MonsGeekDevice>,
    info: DeviceInfo,
    tab: usize,
    selected: usize,
    key_depths: Vec<f32>,
    depth_monitoring: bool,
    last_refresh: Instant,
    status_msg: String,
    connected: bool,
    device_name: String,
    key_count: u8,
    // Trigger settings
    triggers: Option<TriggerSettings>,
    trigger_scroll: usize,
    precision_factor: f32,
    // Keyboard options
    options: Option<KeyboardOptions>,
}

/// Keyboard options state
#[derive(Debug, Clone, Default)]
struct KeyboardOptions {
    os_mode: u8,
    fn_layer: u8,
    anti_mistouch: bool,
    rt_stability: u8,
    wasd_swap: bool,
}

impl App {
    fn new() -> Self {
        Self {
            device: None,
            info: DeviceInfo::default(),
            tab: 0,
            selected: 0,
            key_depths: Vec::new(),
            depth_monitoring: false,
            last_refresh: Instant::now(),
            status_msg: String::new(),
            connected: false,
            device_name: String::new(),
            key_count: 0,
            triggers: None,
            trigger_scroll: 0,
            precision_factor: 100.0, // Default 0.01mm precision
            options: None,
        }
    }

    fn connect(&mut self) -> Result<(), String> {
        match MonsGeekDevice::open() {
            Ok(dev) => {
                // Get device info from definition
                self.device_name = dev.display_name().to_string();
                self.key_count = dev.key_count();
                // Initialize key depths array based on actual key count
                self.key_depths = vec![0.0; self.key_count as usize];
                self.device = Some(dev);
                self.connected = true;
                self.status_msg = format!("Connected to {}", self.device_name);
                Ok(())
            }
            Err(e) => {
                self.connected = false;
                Err(e)
            }
        }
    }

    fn refresh_info(&mut self) {
        if let Some(ref device) = self.device {
            self.info = device.read_info();
            self.precision_factor = MonsGeekDevice::precision_factor_from_version(self.info.version);
            self.last_refresh = Instant::now();
        }
    }

    fn refresh_triggers(&mut self) {
        if let Some(ref device) = self.device {
            self.triggers = device.get_all_triggers();
            if self.triggers.is_some() {
                self.status_msg = "Trigger settings loaded".to_string();
            } else {
                self.status_msg = "Failed to load trigger settings".to_string();
            }
        }
    }

    fn toggle_depth_monitoring(&mut self) {
        if let Some(ref device) = self.device {
            self.depth_monitoring = !self.depth_monitoring;
            device.set_magnetism_report(self.depth_monitoring);
            self.status_msg = if self.depth_monitoring {
                "Key depth monitoring ENABLED".to_string()
            } else {
                "Key depth monitoring DISABLED".to_string()
            };
        }
    }

    fn set_led_mode(&mut self, mode: u8) {
        if let Some(ref device) = self.device {
            if device.set_led(
                mode,
                self.info.led_brightness,
                4 - self.info.led_speed.min(4),
                self.info.led_r,
                self.info.led_g,
                self.info.led_b,
                self.info.led_dazzle,
            ) {
                self.info.led_mode = mode;
                self.status_msg = format!("LED mode: {}", cmd::led_mode_name(mode));
            }
        }
    }

    fn set_brightness(&mut self, brightness: u8) {
        if let Some(ref device) = self.device {
            let brightness = brightness.min(4);
            if device.set_led(
                self.info.led_mode,
                brightness,
                4 - self.info.led_speed.min(4),
                self.info.led_r,
                self.info.led_g,
                self.info.led_b,
                self.info.led_dazzle,
            ) {
                self.info.led_brightness = brightness;
                self.status_msg = format!("Brightness: {}/4", brightness);
            }
        }
    }

    fn set_speed(&mut self, speed: u8) {
        if let Some(ref device) = self.device {
            let speed = speed.min(4);
            if device.set_led(
                self.info.led_mode,
                self.info.led_brightness,
                speed,
                self.info.led_r,
                self.info.led_g,
                self.info.led_b,
                self.info.led_dazzle,
            ) {
                self.info.led_speed = 4 - speed;
                self.status_msg = format!("Speed: {}/4", speed);
            }
        }
    }

    fn set_profile(&mut self, profile: u8) {
        if let Some(ref device) = self.device {
            if device.set_profile(profile) {
                self.info.profile = profile;
                self.status_msg = format!("Switched to profile {}", profile + 1);
                // Refresh info to get profile-specific settings
                self.refresh_info();
            } else {
                self.status_msg = format!("Failed to set profile {}", profile + 1);
            }
        }
    }

    fn set_color(&mut self, r: u8, g: u8, b: u8) {
        if let Some(ref device) = self.device {
            if device.set_led(
                self.info.led_mode,
                self.info.led_brightness,
                4 - self.info.led_speed.min(4),
                r, g, b,
                self.info.led_dazzle,
            ) {
                self.info.led_r = r;
                self.info.led_g = g;
                self.info.led_b = b;
                self.status_msg = format!("Color: #{:02X}{:02X}{:02X}", r, g, b);
            }
        }
    }

    fn toggle_dazzle(&mut self) {
        if let Some(ref device) = self.device {
            let new_dazzle = !self.info.led_dazzle;
            if device.set_led(
                self.info.led_mode,
                self.info.led_brightness,
                4 - self.info.led_speed.min(4),
                self.info.led_r,
                self.info.led_g,
                self.info.led_b,
                new_dazzle,
            ) {
                self.info.led_dazzle = new_dazzle;
                self.status_msg = format!("Dazzle: {}", if new_dazzle { "ON" } else { "OFF" });
            }
        }
    }

    // Side LED methods
    fn set_side_mode(&mut self, mode: u8) {
        if let Some(ref device) = self.device {
            if device.set_side_led(
                mode,
                self.info.side_brightness,
                4 - self.info.side_speed.min(4),
                self.info.side_r,
                self.info.side_g,
                self.info.side_b,
                self.info.side_dazzle,
            ) {
                self.info.side_mode = mode;
                self.status_msg = format!("Side LED mode: {}", cmd::led_mode_name(mode));
            }
        }
    }

    fn set_side_brightness(&mut self, brightness: u8) {
        if let Some(ref device) = self.device {
            let brightness = brightness.min(4);
            if device.set_side_led(
                self.info.side_mode,
                brightness,
                4 - self.info.side_speed.min(4),
                self.info.side_r,
                self.info.side_g,
                self.info.side_b,
                self.info.side_dazzle,
            ) {
                self.info.side_brightness = brightness;
                self.status_msg = format!("Side brightness: {}/4", brightness);
            }
        }
    }

    fn set_side_speed(&mut self, speed: u8) {
        if let Some(ref device) = self.device {
            let speed = speed.min(4);
            if device.set_side_led(
                self.info.side_mode,
                self.info.side_brightness,
                speed,
                self.info.side_r,
                self.info.side_g,
                self.info.side_b,
                self.info.side_dazzle,
            ) {
                self.info.side_speed = 4 - speed;
                self.status_msg = format!("Side speed: {}/4", speed);
            }
        }
    }

    fn set_side_color(&mut self, r: u8, g: u8, b: u8) {
        if let Some(ref device) = self.device {
            if device.set_side_led(
                self.info.side_mode,
                self.info.side_brightness,
                4 - self.info.side_speed.min(4),
                r, g, b,
                self.info.side_dazzle,
            ) {
                self.info.side_r = r;
                self.info.side_g = g;
                self.info.side_b = b;
                self.status_msg = format!("Side color: #{:02X}{:02X}{:02X}", r, g, b);
            }
        }
    }

    fn toggle_side_dazzle(&mut self) {
        if let Some(ref device) = self.device {
            let new_dazzle = !self.info.side_dazzle;
            if device.set_side_led(
                self.info.side_mode,
                self.info.side_brightness,
                4 - self.info.side_speed.min(4),
                self.info.side_r,
                self.info.side_g,
                self.info.side_b,
                new_dazzle,
            ) {
                self.info.side_dazzle = new_dazzle;
                self.status_msg = format!("Side dazzle: {}", if new_dazzle { "ON" } else { "OFF" });
            }
        }
    }

    fn set_all_key_modes(&mut self, mode: u8) {
        if let (Some(ref device), Some(ref mut triggers)) = (&self.device, &mut self.triggers) {
            let key_count = triggers.key_modes.len();
            let modes: Vec<u8> = vec![mode; key_count];
            if device.set_key_modes(&modes) {
                triggers.key_modes = modes;
                self.status_msg = format!("All keys set to {}", magnetism::mode_name(mode));
            } else {
                self.status_msg = "Failed to set key modes".to_string();
            }
        }
    }

    fn refresh_options(&mut self) {
        if let Some(ref device) = self.device {
            if let Some((os_mode, fn_layer, anti_mistouch, rt_stability, wasd_swap)) = device.get_options() {
                self.options = Some(KeyboardOptions {
                    os_mode,
                    fn_layer,
                    anti_mistouch,
                    rt_stability,
                    wasd_swap,
                });
                self.status_msg = "Keyboard options loaded".to_string();
            } else {
                self.status_msg = "Failed to load keyboard options".to_string();
            }
        }
    }

    fn save_options(&mut self) {
        if let (Some(ref device), Some(ref opts)) = (&self.device, &self.options) {
            if device.set_options(opts.fn_layer, opts.anti_mistouch, opts.rt_stability, opts.wasd_swap) {
                self.status_msg = "Options saved".to_string();
            } else {
                self.status_msg = "Failed to save options".to_string();
            }
        }
    }

    fn set_fn_layer(&mut self, layer: u8) {
        let layer = layer.min(3);
        if let Some(ref mut opts) = self.options {
            opts.fn_layer = layer;
        }
        self.save_options();
        self.status_msg = format!("Fn layer: {}", layer);
    }

    fn toggle_wasd_swap(&mut self) {
        let new_val = self.options.as_ref().map(|o| !o.wasd_swap).unwrap_or(false);
        if let Some(ref mut opts) = self.options {
            opts.wasd_swap = new_val;
        }
        self.save_options();
        self.status_msg = format!("WASD swap: {}", if new_val { "ON" } else { "OFF" });
    }

    fn toggle_anti_mistouch(&mut self) {
        let new_val = self.options.as_ref().map(|o| !o.anti_mistouch).unwrap_or(false);
        if let Some(ref mut opts) = self.options {
            opts.anti_mistouch = new_val;
        }
        self.save_options();
        self.status_msg = format!("Anti-mistouch: {}", if new_val { "ON" } else { "OFF" });
    }

    fn set_rt_stability(&mut self, value: u8) {
        let value = value.min(125);
        if let Some(ref mut opts) = self.options {
            opts.rt_stability = value;
        }
        self.save_options();
        self.status_msg = format!("RT stability: {}ms", value);
    }

    fn read_input_reports(&mut self) {
        if !self.depth_monitoring {
            return;
        }
        if let Some(ref device) = self.device {
            // Non-blocking read of input reports
            while let Some(buf) = device.read_input(10) {
                if buf.len() > 2 && buf[0] == cmd::SET_MAGNETISM_REPORT {
                    let precision = MonsGeekDevice::precision_factor(self.info.precision);
                    let depth = ((buf[1] as u16) | ((buf[2] as u16) << 8)) as f32 / precision;
                    // Store depth - simplified mapping
                    if (buf[1] as usize) < self.key_depths.len() {
                        self.key_depths[buf[1] as usize] = depth;
                    }
                }
            }
        }
    }
}

fn main() -> io::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let mut app = App::new();

    // Try to connect
    if let Err(e) = app.connect() {
        app.status_msg = e;
    } else {
        app.refresh_info();
    }

    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui(f, &app))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Tab => {
                            app.tab = (app.tab + 1) % 5;
                            app.selected = 0;
                            app.trigger_scroll = 0;
                            // Auto-refresh when entering Options or Triggers tab
                            if app.tab == 3 && app.triggers.is_none() {
                                app.refresh_triggers();
                            } else if app.tab == 4 && app.options.is_none() {
                                app.refresh_options();
                            }
                        }
                        KeyCode::BackTab => {
                            app.tab = if app.tab == 0 { 4 } else { app.tab - 1 };
                            app.selected = 0;
                            app.trigger_scroll = 0;
                            // Auto-refresh when entering Options or Triggers tab
                            if app.tab == 3 && app.triggers.is_none() {
                                app.refresh_triggers();
                            } else if app.tab == 4 && app.options.is_none() {
                                app.refresh_options();
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.tab == 3 {
                                // Scroll trigger list
                                if app.trigger_scroll > 0 {
                                    app.trigger_scroll -= 1;
                                }
                            } else if app.selected > 0 {
                                app.selected -= 1;
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.tab == 3 {
                                // Scroll trigger list
                                let max_scroll = app.triggers.as_ref()
                                    .map(|t| t.key_modes.len().saturating_sub(15))
                                    .unwrap_or(0);
                                if app.trigger_scroll < max_scroll {
                                    app.trigger_scroll += 1;
                                }
                            } else {
                                app.selected += 1;
                            }
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            if app.tab == 1 {
                                let step: u8 = if key.modifiers.contains(event::KeyModifiers::SHIFT) { 10 } else { 1 };
                                match app.selected {
                                    // Main LED
                                    0 if app.info.led_mode > 0 => app.set_led_mode(app.info.led_mode - 1),
                                    1 if app.info.led_brightness > 0 => app.set_brightness(app.info.led_brightness - 1),
                                    2 => {
                                        let current = 4 - app.info.led_speed.min(4);
                                        if current > 0 {
                                            app.set_speed(current - 1);
                                        }
                                    }
                                    3 => { // Red
                                        let r = app.info.led_r.saturating_sub(step);
                                        app.set_color(r, app.info.led_g, app.info.led_b);
                                    }
                                    4 => { // Green
                                        let g = app.info.led_g.saturating_sub(step);
                                        app.set_color(app.info.led_r, g, app.info.led_b);
                                    }
                                    5 => { // Blue
                                        let b = app.info.led_b.saturating_sub(step);
                                        app.set_color(app.info.led_r, app.info.led_g, b);
                                    }
                                    7 => app.toggle_dazzle(), // Dazzle
                                    // Side LED (8 is separator)
                                    9 if app.info.side_mode > 0 => app.set_side_mode(app.info.side_mode - 1),
                                    10 if app.info.side_brightness > 0 => app.set_side_brightness(app.info.side_brightness - 1),
                                    11 => {
                                        let current = 4 - app.info.side_speed.min(4);
                                        if current > 0 {
                                            app.set_side_speed(current - 1);
                                        }
                                    }
                                    12 => { // Side Red
                                        let r = app.info.side_r.saturating_sub(step);
                                        app.set_side_color(r, app.info.side_g, app.info.side_b);
                                    }
                                    13 => { // Side Green
                                        let g = app.info.side_g.saturating_sub(step);
                                        app.set_side_color(app.info.side_r, g, app.info.side_b);
                                    }
                                    14 => { // Side Blue
                                        let b = app.info.side_b.saturating_sub(step);
                                        app.set_side_color(app.info.side_r, app.info.side_g, b);
                                    }
                                    15 => app.toggle_side_dazzle(), // Side Dazzle
                                    _ => {}
                                }
                            } else if app.tab == 4 {
                                // Options tab
                                if let Some(ref opts) = app.options.clone() {
                                    match app.selected {
                                        0 if opts.fn_layer > 0 => app.set_fn_layer(opts.fn_layer - 1),
                                        1 => app.toggle_wasd_swap(),
                                        2 => app.toggle_anti_mistouch(),
                                        3 if opts.rt_stability >= 25 => app.set_rt_stability(opts.rt_stability - 25),
                                        _ => {}
                                    }
                                }
                            }
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            if app.tab == 1 {
                                let step: u8 = if key.modifiers.contains(event::KeyModifiers::SHIFT) { 10 } else { 1 };
                                match app.selected {
                                    // Main LED
                                    0 if app.info.led_mode < cmd::LED_MODE_MAX => app.set_led_mode(app.info.led_mode + 1),
                                    1 if app.info.led_brightness < 4 => app.set_brightness(app.info.led_brightness + 1),
                                    2 => {
                                        let current = 4 - app.info.led_speed.min(4);
                                        if current < 4 {
                                            app.set_speed(current + 1);
                                        }
                                    }
                                    3 => { // Red
                                        let r = app.info.led_r.saturating_add(step);
                                        app.set_color(r, app.info.led_g, app.info.led_b);
                                    }
                                    4 => { // Green
                                        let g = app.info.led_g.saturating_add(step);
                                        app.set_color(app.info.led_r, g, app.info.led_b);
                                    }
                                    5 => { // Blue
                                        let b = app.info.led_b.saturating_add(step);
                                        app.set_color(app.info.led_r, app.info.led_g, b);
                                    }
                                    7 => app.toggle_dazzle(), // Dazzle
                                    // Side LED (8 is separator)
                                    9 if app.info.side_mode < cmd::LED_MODE_MAX => app.set_side_mode(app.info.side_mode + 1),
                                    10 if app.info.side_brightness < 4 => app.set_side_brightness(app.info.side_brightness + 1),
                                    11 => {
                                        let current = 4 - app.info.side_speed.min(4);
                                        if current < 4 {
                                            app.set_side_speed(current + 1);
                                        }
                                    }
                                    12 => { // Side Red
                                        let r = app.info.side_r.saturating_add(step);
                                        app.set_side_color(r, app.info.side_g, app.info.side_b);
                                    }
                                    13 => { // Side Green
                                        let g = app.info.side_g.saturating_add(step);
                                        app.set_side_color(app.info.side_r, g, app.info.side_b);
                                    }
                                    14 => { // Side Blue
                                        let b = app.info.side_b.saturating_add(step);
                                        app.set_side_color(app.info.side_r, app.info.side_g, b);
                                    }
                                    15 => app.toggle_side_dazzle(), // Side Dazzle
                                    _ => {}
                                }
                            } else if app.tab == 4 {
                                // Options tab
                                if let Some(ref opts) = app.options.clone() {
                                    match app.selected {
                                        0 if opts.fn_layer < 3 => app.set_fn_layer(opts.fn_layer + 1),
                                        1 => app.toggle_wasd_swap(),
                                        2 => app.toggle_anti_mistouch(),
                                        3 if opts.rt_stability < 125 => app.set_rt_stability(opts.rt_stability + 25),
                                        _ => {}
                                    }
                                }
                            }
                        }
                        KeyCode::Char('r') => {
                            app.refresh_info();
                            if app.tab == 3 {
                                app.refresh_triggers();
                            } else if app.tab == 4 {
                                app.refresh_options();
                            } else {
                                app.status_msg = "Refreshed device info".to_string();
                            }
                        }
                        KeyCode::Char('m') => {
                            app.toggle_depth_monitoring();
                        }
                        KeyCode::Char('c') => {
                            if let Err(e) = app.connect() {
                                app.status_msg = e;
                            } else {
                                app.refresh_info();
                            }
                        }
                        // Profile switching with number keys 1-4
                        KeyCode::Char('1') => app.set_profile(0),
                        KeyCode::Char('2') => app.set_profile(1),
                        KeyCode::Char('3') => app.set_profile(2),
                        KeyCode::Char('4') => app.set_profile(3),
                        // Page up/down for fast trigger scrolling
                        KeyCode::PageUp => {
                            if app.tab == 3 {
                                app.trigger_scroll = app.trigger_scroll.saturating_sub(15);
                            }
                        }
                        KeyCode::PageDown => {
                            if app.tab == 3 {
                                let max_scroll = app.triggers.as_ref()
                                    .map(|t| t.key_modes.len().saturating_sub(15))
                                    .unwrap_or(0);
                                app.trigger_scroll = (app.trigger_scroll + 15).min(max_scroll);
                            }
                        }
                        // Key mode switching on Triggers tab (Shift+key sets all)
                        KeyCode::Char('n') if app.tab == 3 => {
                            app.set_all_key_modes(magnetism::MODE_NORMAL);
                        }
                        KeyCode::Char('t') if app.tab == 3 => {
                            app.set_all_key_modes(magnetism::MODE_RAPID_TRIGGER);
                        }
                        KeyCode::Char('d') if app.tab == 3 => {
                            app.set_all_key_modes(magnetism::MODE_DKS);
                        }
                        KeyCode::Char('s') if app.tab == 3 => {
                            app.set_all_key_modes(magnetism::MODE_SNAPTAP);
                        }
                        _ => {}
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.read_input_reports();
            last_tick = Instant::now();
        }
    }

    // Cleanup
    if app.depth_monitoring {
        if let Some(ref device) = app.device {
            device.set_magnetism_report(false);
        }
    }
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Length(3),  // Tabs
            Constraint::Min(10),    // Content
            Constraint::Length(3),  // Status bar
        ])
        .split(f.area());

    // Title - show device name if connected, otherwise generic title
    let title_text = if app.connected && !app.device_name.is_empty() {
        format!("{} - Configuration Tool", app.device_name)
    } else {
        "MonsGeek/Akko Keyboard - Configuration Tool".to_string()
    };
    let title = Paragraph::new(title_text)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    // Tabs
    let tabs = Tabs::new(vec!["Device Info", "LED Settings", "Key Depth", "Triggers", "Options"])
        .select(app.tab)
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL).title("Tabs [Tab/Shift+Tab]"));
    f.render_widget(tabs, chunks[1]);

    // Content based on tab
    match app.tab {
        0 => render_device_info(f, app, chunks[2]),
        1 => render_led_settings(f, app, chunks[2]),
        2 => render_depth_monitor(f, app, chunks[2]),
        3 => render_trigger_settings(f, app, chunks[2]),
        4 => render_options(f, app, chunks[2]),
        _ => {}
    }

    // Status bar
    let status_color = if app.connected { Color::Green } else { Color::Red };
    let conn_status = if app.connected { "Connected" } else { "Disconnected" };
    let profile_str = if app.connected {
        format!(" P{}", app.info.profile + 1)
    } else {
        String::new()
    };
    let status = Paragraph::new(format!(
        " [{}{}] {} | q:Quit r:Refresh c:Connect m:Monitor 1-4:Profile | {}",
        conn_status, profile_str, app.status_msg,
        if app.depth_monitoring { "MONITORING" } else { "" }
    ))
    .style(Style::default().fg(status_color))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(status, chunks[3]);
}

fn render_device_info(f: &mut Frame, app: &App, area: Rect) {
    let info = &app.info;

    let text = vec![
        Line::from(vec![
            Span::raw("Device:         "),
            Span::styled(&app.device_name, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::raw("Key Count:      "),
            Span::styled(format!("{}", app.key_count), Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::raw("Device ID:      "),
            Span::styled(format!("{} (0x{:04X})", info.device_id, info.device_id), Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::raw("Firmware:       "),
            Span::styled(format!("v{}.{:02}", info.version / 100, info.version % 100), Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::raw("Profile:        "),
            Span::styled(format!("{}", info.profile), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::raw("Debounce:       "),
            Span::styled(format!("{} ms", info.debounce), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::raw("Fn Layer:       "),
            Span::styled(format!("{}", info.fn_layer), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::raw("WASD Swap:      "),
            Span::styled(if info.wasd_swap { "Yes" } else { "No" }, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::raw("Precision:      "),
            Span::styled(MonsGeekDevice::precision_str(info.precision), Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::raw("Sleep:          "),
            Span::styled(format!("{} sec ({} min)", info.sleep_seconds, info.sleep_seconds / 60), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("LED Mode:       "),
            Span::styled(
                format!("{} ({})", info.led_mode, cmd::led_mode_name(info.led_mode)),
                Style::default().fg(Color::Magenta)
            ),
        ]),
        Line::from(vec![
            Span::raw("LED Color:      "),
            Span::styled(
                format!("RGB({}, {}, {}) #{:02X}{:02X}{:02X}",
                    info.led_r, info.led_g, info.led_b,
                    info.led_r, info.led_g, info.led_b),
                Style::default().fg(Color::Rgb(info.led_r, info.led_g, info.led_b))
            ),
        ]),
        Line::from(vec![
            Span::raw("Brightness:     "),
            Span::styled(format!("{}/4", info.led_brightness), Style::default().fg(Color::Magenta)),
        ]),
        Line::from(vec![
            Span::raw("Speed:          "),
            Span::styled(format!("{}/4", 4 - info.led_speed.min(4)), Style::default().fg(Color::Magenta)),
        ]),
    ];

    let para = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Device Information [r to refresh]"));
    f.render_widget(para, area);
}

fn render_led_settings(f: &mut Frame, app: &App, area: Rect) {
    let info = &app.info;
    let speed = 4 - info.led_speed.min(4);

    // Helper to create RGB bar visualization
    let rgb_bar = |val: u8| -> String {
        let bars = (val as usize * 16 / 255).min(16);
        format!("{:3} {}", val, "█".repeat(bars))
    };

    let items: Vec<ListItem> = vec![
        ListItem::new(Line::from(vec![
            Span::raw("Mode:       "),
            Span::styled(
                format!("< {} ({}) >", info.led_mode, cmd::led_mode_name(info.led_mode)),
                Style::default().fg(Color::Yellow)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Brightness: "),
            Span::styled(
                format!("< {}/4 >  {}", info.led_brightness, "█".repeat(info.led_brightness as usize)),
                Style::default().fg(Color::Yellow)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Speed:      "),
            Span::styled(
                format!("< {}/4 >  {}", speed, "█".repeat(speed as usize)),
                Style::default().fg(Color::Yellow)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Red:        "),
            Span::styled(
                format!("< {} >", rgb_bar(info.led_r)),
                Style::default().fg(Color::Red)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Green:      "),
            Span::styled(
                format!("< {} >", rgb_bar(info.led_g)),
                Style::default().fg(Color::Green)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Blue:       "),
            Span::styled(
                format!("< {} >", rgb_bar(info.led_b)),
                Style::default().fg(Color::Blue)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Preview:    "),
            Span::styled(
                format!("████████ #{:02X}{:02X}{:02X}", info.led_r, info.led_g, info.led_b),
                Style::default().fg(Color::Rgb(info.led_r, info.led_g, info.led_b))
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Dazzle:     "),
            Span::styled(
                if info.led_dazzle { "< ON (rainbow) >" } else { "< OFF >" },
                Style::default().fg(if info.led_dazzle { Color::Magenta } else { Color::Gray })
            ),
        ])),
        // Side LED section
        ListItem::new(Line::from(vec![
            Span::styled("─── Side LEDs (Side Lights) ───", Style::default().fg(Color::DarkGray)),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Mode:       "),
            Span::styled(
                format!("< {} ({}) >", info.side_mode, cmd::led_mode_name(info.side_mode)),
                Style::default().fg(Color::Cyan)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Brightness: "),
            Span::styled(
                format!("< {}/4 >  {}", info.side_brightness, "█".repeat(info.side_brightness as usize)),
                Style::default().fg(Color::Cyan)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Speed:      "),
            Span::styled(
                format!("< {}/4 >  {}", 4 - info.side_speed.min(4), "█".repeat((4 - info.side_speed.min(4)) as usize)),
                Style::default().fg(Color::Cyan)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Red:        "),
            Span::styled(
                format!("< {} >", rgb_bar(info.side_r)),
                Style::default().fg(Color::Red)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Green:      "),
            Span::styled(
                format!("< {} >", rgb_bar(info.side_g)),
                Style::default().fg(Color::Green)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Blue:       "),
            Span::styled(
                format!("< {} >", rgb_bar(info.side_b)),
                Style::default().fg(Color::Blue)
            ),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Dazzle:     "),
            Span::styled(
                if info.side_dazzle { "< ON (rainbow) >" } else { "< OFF >" },
                Style::default().fg(if info.side_dazzle { Color::Magenta } else { Color::Gray })
            ),
        ])),
    ];

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("LED Settings [←/→ adjust, ↑/↓ select, ±10 with Shift]"))
        .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");

    let mut state = ListState::default();
    state.select(Some(app.selected.min(15)));
    f.render_stateful_widget(list, area, &mut state);
}

fn render_depth_monitor(f: &mut Frame, app: &App, area: Rect) {
    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(5)])
        .split(area);

    // Status and instructions
    let status_text = if app.depth_monitoring {
        vec![
            Line::from(Span::styled("Key depth monitoring is ACTIVE", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
            Line::from("Press keys to see real-time depth values"),
            Line::from("Press 'm' to stop monitoring"),
        ]
    } else {
        vec![
            Line::from(Span::styled("Key depth monitoring is OFF", Style::default().fg(Color::Yellow))),
            Line::from("Press 'm' to start monitoring"),
            Line::from("This enables real-time key depth reporting via HID input reports"),
        ]
    };
    let status = Paragraph::new(status_text)
        .block(Block::default().borders(Borders::ALL).title("Monitor Status"));
    f.render_widget(status, inner[0]);

    // Key depth visualization
    let mut bars: Vec<(&str, u64)> = vec![];
    let key_names = [
        "Esc", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "=", "Bsp",
        "Tab", "Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P", "[", "]", "\\",
        "Cap", "A", "S", "D", "F", "G", "H", "J", "K", "L", ";", "'", "Ent",
        "Shf", "Z", "X", "C", "V", "B", "N", "M", ",", ".", "/", "Shf",
        "Ctl", "Win", "Alt", "Space", "Alt", "Fn", "Ctl",
    ];

    for (i, name) in key_names.iter().enumerate() {
        if i < app.key_depths.len() {
            let depth_pct = (app.key_depths[i] * 25.0) as u64; // Scale to percentage (4mm max = 100%)
            bars.push((*name, depth_pct.min(100)));
        }
    }

    // Show first row of keys as bar chart
    let chart_data: Vec<(&str, u64)> = bars.into_iter().take(14).collect();
    let chart = BarChart::default()
        .block(Block::default().borders(Borders::ALL).title("Key Depths (top row) - Press keys to see activity"))
        .bar_width(3)
        .bar_gap(1)
        .bar_style(Style::default().fg(Color::Cyan))
        .value_style(Style::default().fg(Color::White))
        .data(&chart_data);
    f.render_widget(chart, inner[1]);
}

fn render_trigger_settings(f: &mut Frame, app: &App, area: Rect) {
    // Split into summary and detail areas
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),   // Summary
            Constraint::Min(10),     // Key list
        ])
        .split(area);

    // Summary section
    let factor = app.precision_factor;
    let precision_str = if factor >= 200.0 { "0.005mm" }
        else if factor >= 100.0 { "0.01mm" }
        else { "0.1mm" };

    let summary = if let Some(ref triggers) = app.triggers {
        // Decode first key values
        let decode_u16 = |data: &[u8], idx: usize| -> u16 {
            if idx * 2 + 1 < data.len() {
                u16::from_le_bytes([data[idx * 2], data[idx * 2 + 1]])
            } else {
                0
            }
        };
        let first_press = decode_u16(&triggers.press_travel, 0);
        let first_lift = decode_u16(&triggers.lift_travel, 0);
        let first_rt_press = decode_u16(&triggers.rt_press, 0);
        let first_rt_lift = decode_u16(&triggers.rt_lift, 0);
        let first_mode = triggers.key_modes.first().copied().unwrap_or(0);
        let num_keys = triggers.key_modes.len().min(triggers.press_travel.len() / 2);

        vec![
            Line::from(vec![
                Span::styled("Precision: ", Style::default().fg(Color::Gray)),
                Span::styled(precision_str, Style::default().fg(Color::Green)),
                Span::raw("  |  "),
                Span::styled("Keys: ", Style::default().fg(Color::Gray)),
                Span::styled(format!("{}", num_keys), Style::default().fg(Color::Green)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Global Settings (all keys same): ", Style::default().add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::raw("  Actuation: "),
                Span::styled(format!("{:.2}mm", first_press as f32 / factor), Style::default().fg(Color::Cyan)),
                Span::raw("  |  Release: "),
                Span::styled(format!("{:.2}mm", first_lift as f32 / factor), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::raw("  RT Press: "),
                Span::styled(format!("{:.2}mm", first_rt_press as f32 / factor), Style::default().fg(Color::Yellow)),
                Span::raw("   |  RT Release: "),
                Span::styled(format!("{:.2}mm", first_rt_lift as f32 / factor), Style::default().fg(Color::Yellow)),
            ]),
            Line::from(vec![
                Span::raw("  Mode: "),
                Span::styled(magnetism::mode_name(first_mode), Style::default().fg(Color::Magenta)),
            ]),
        ]
    } else {
        vec![
            Line::from(Span::styled("No trigger data loaded", Style::default().fg(Color::Red))),
            Line::from(""),
            Line::from("Press 'r' to load trigger settings from device"),
        ]
    };

    let summary_block = Paragraph::new(summary)
        .block(Block::default().borders(Borders::ALL).title("Trigger Settings Summary"));
    f.render_widget(summary_block, chunks[0]);

    // Key list section
    if let Some(ref triggers) = app.triggers {
        let decode_u16 = |data: &[u8], idx: usize| -> u16 {
            if idx * 2 + 1 < data.len() {
                u16::from_le_bytes([data[idx * 2], data[idx * 2 + 1]])
            } else {
                0
            }
        };

        let num_keys = triggers.key_modes.len().min(triggers.press_travel.len() / 2);

        // Build rows for table
        let rows: Vec<Row> = (0..num_keys)
            .skip(app.trigger_scroll)
            .take(15) // Show 15 keys at a time
            .map(|i| {
                let press = decode_u16(&triggers.press_travel, i);
                let lift = decode_u16(&triggers.lift_travel, i);
                let rt_p = decode_u16(&triggers.rt_press, i);
                let rt_l = decode_u16(&triggers.rt_lift, i);
                let mode = triggers.key_modes.get(i).copied().unwrap_or(0);

                Row::new(vec![
                    Cell::from(format!("{:3}", i)),
                    Cell::from(format!("{:.2}", press as f32 / factor)),
                    Cell::from(format!("{:.2}", lift as f32 / factor)),
                    Cell::from(format!("{:.2}", rt_p as f32 / factor)),
                    Cell::from(format!("{:.2}", rt_l as f32 / factor)),
                    Cell::from(magnetism::mode_name(mode)),
                ])
            })
            .collect();

        let header = Row::new(vec!["Key", "Act(mm)", "Rel(mm)", "RT↓(mm)", "RT↑(mm)", "Mode"])
            .style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan));

        let table = Table::new(rows, [
            Constraint::Length(4),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(14),
        ])
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(format!(
            "Per-Key [{}-{}] n:Normal t:RT d:DKS s:SnapTap",
            app.trigger_scroll,
            (app.trigger_scroll + 15).min(num_keys)
        )));

        f.render_widget(table, chunks[1]);
    } else {
        let help = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("Controls:", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  r - Reload trigger settings from device"),
            Line::from("  ↑/↓ - Scroll through keys"),
            Line::from(""),
            Line::from(Span::styled("Mode Switching (all keys):", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("  n - Normal mode"),
            Line::from("  t - Rapid Trigger mode"),
            Line::from("  d - DKS mode"),
            Line::from("  s - SnapTap mode"),
        ])
        .block(Block::default().borders(Borders::ALL).title("Per-Key Settings"));
        f.render_widget(help, chunks[1]);
    }
}

fn render_options(f: &mut Frame, app: &App, area: Rect) {
    if let Some(ref opts) = app.options {
        let os_mode_str = match opts.os_mode {
            0 => "Windows",
            1 => "macOS",
            2 => "Linux",
            _ => "Unknown",
        };

        let items: Vec<ListItem> = vec![
            ListItem::new(Line::from(vec![
                Span::raw("Fn Layer:       "),
                Span::styled(
                    format!("< {} >", opts.fn_layer),
                    Style::default().fg(Color::Yellow)
                ),
                Span::styled("  (0-3)", Style::default().fg(Color::DarkGray)),
            ])),
            ListItem::new(Line::from(vec![
                Span::raw("WASD Swap:      "),
                Span::styled(
                    if opts.wasd_swap { "< ON >" } else { "< OFF >" },
                    Style::default().fg(if opts.wasd_swap { Color::Green } else { Color::Gray })
                ),
                Span::styled("  (swap WASD/Arrow keys)", Style::default().fg(Color::DarkGray)),
            ])),
            ListItem::new(Line::from(vec![
                Span::raw("Anti-Mistouch:  "),
                Span::styled(
                    if opts.anti_mistouch { "< ON >" } else { "< OFF >" },
                    Style::default().fg(if opts.anti_mistouch { Color::Green } else { Color::Gray })
                ),
                Span::styled("  (prevent accidental key presses)", Style::default().fg(Color::DarkGray)),
            ])),
            ListItem::new(Line::from(vec![
                Span::raw("RT Stability:   "),
                Span::styled(
                    format!("< {}ms >", opts.rt_stability),
                    Style::default().fg(Color::Cyan)
                ),
                Span::styled("  (0-125ms, delay for stability)", Style::default().fg(Color::DarkGray)),
            ])),
            ListItem::new(Line::from("")),
            ListItem::new(Line::from(vec![
                Span::styled("Read-Only Info:", Style::default().add_modifier(Modifier::BOLD)),
            ])),
            ListItem::new(Line::from(vec![
                Span::raw("OS Mode:        "),
                Span::styled(os_mode_str, Style::default().fg(Color::Magenta)),
            ])),
        ];

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Keyboard Options [←/→ adjust, ↑/↓ select, Enter to toggle]"))
            .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
            .highlight_symbol("> ");

        let mut state = ListState::default();
        state.select(Some(app.selected.min(3)));
        f.render_stateful_widget(list, area, &mut state);
    } else {
        let help = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("No options loaded", Style::default().fg(Color::Red))),
            Line::from(""),
            Line::from("Press 'r' to load keyboard options from device"),
        ])
        .block(Block::default().borders(Borders::ALL).title("Keyboard Options"));
        f.render_widget(help, area);
    }
}

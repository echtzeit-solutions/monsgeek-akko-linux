//! MonsGeek Magnetic Key-to-Joystick Mapper
//!
//! Main entry point and TUI run loop.
//! Uses event-driven depth from the keyboard's broadcast channel
//! with coalescing drain for low-latency axis updates.

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use futures::StreamExt;
use ratatui::prelude::*;
use std::collections::HashMap;
use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use monsgeek_joystick::config::JoystickConfig;
use monsgeek_joystick::joystick::VirtualJoystick;
use monsgeek_joystick::mapper::AxisMapper;
use monsgeek_joystick::tui::app::{App, AppMode, JoystickStatus, KeyboardStatus};
use monsgeek_joystick::tui::render;

use monsgeek_keyboard::KeyboardInterface;
use monsgeek_transport::{list_devices_sync, open_device_sync, TimestampedEvent, VendorEvent};

#[derive(Parser)]
#[command(name = "monsgeek-joystick")]
#[command(about = "Magnetic key-to-joystick mapper for MonsGeek HE keyboards")]
struct Cli {
    /// Config file path (default: ~/.config/monsgeek/joystick.toml)
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Run without TUI (headless mode)
    #[arg(long)]
    headless: bool,

    /// Log level (error, warn, info, debug, trace)
    #[arg(long, default_value = "info")]
    log_level: String,
}

/// Encapsulates a live keyboard connection with event subscription.
struct KeyboardConnection {
    keyboard: KeyboardInterface,
    precision_factor: f64,
    event_rx: broadcast::Receiver<TimestampedEvent>,
}

/// Connect to a keyboard, get precision, start magnetism reporting,
/// and subscribe to the event broadcast channel.
///
/// Returns `None` if no keyboard is found or connection fails.
async fn connect_keyboard() -> Option<KeyboardConnection> {
    let devices = match list_devices_sync() {
        Ok(d) if !d.is_empty() => d,
        Ok(_) => {
            debug!("No supported keyboard found");
            return None;
        }
        Err(e) => {
            warn!("Failed to list devices: {}", e);
            return None;
        }
    };

    let transport = match open_device_sync(&devices[0]) {
        Ok(t) => t,
        Err(e) => {
            warn!("Failed to open device: {}", e);
            return None;
        }
    };

    let info = transport.device_info();
    info!(
        "Connected to keyboard: {} ({:04x}:{:04x})",
        info.product_name.as_deref().unwrap_or("Unknown"),
        info.vid,
        info.pid
    );

    let keyboard = KeyboardInterface::new(
        transport.inner().clone(),
        monsgeek_keyboard::KEY_COUNT_M1_V5,
        true,
    );

    let precision_factor = match keyboard.get_precision().await {
        Ok(precision) => {
            let factor = precision.factor();
            info!("Precision: {} (factor: {})", precision.as_str(), factor);
            factor
        }
        Err(e) => {
            warn!("Failed to get precision: {}", e);
            return None;
        }
    };

    if let Err(e) = keyboard.start_magnetism_report().await {
        warn!("Failed to start magnetism report: {}", e);
        return None;
    }
    info!("Started magnetism reporting");

    let event_rx = match keyboard.subscribe_events() {
        Some(rx) => rx,
        None => {
            warn!("Event subscription not supported (no input endpoint)");
            return None;
        }
    };

    Some(KeyboardConnection {
        keyboard,
        precision_factor,
        event_rx,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cli.log_level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    // Load config
    let config_path = cli.config.unwrap_or_else(JoystickConfig::default_path);
    info!("Loading config from {:?}", config_path);
    let config = JoystickConfig::load(&config_path)?;

    if cli.headless {
        run_headless(config, config_path).await
    } else {
        run_tui(config, config_path).await
    }
}

/// Drain all pending depth events from the broadcast channel, coalescing
/// by key_index (keeping only the latest depth per key).
///
/// Returns `true` if the channel closed during drain.
fn drain_depth_events(
    rx: &mut broadcast::Receiver<TimestampedEvent>,
    pending_depths: &mut HashMap<u8, u16>,
) -> bool {
    loop {
        match rx.try_recv() {
            Ok(ts) => {
                if let VendorEvent::KeyDepth {
                    key_index,
                    depth_raw,
                } = ts.event
                {
                    pending_depths.insert(key_index, depth_raw);
                }
                // Ignore non-depth events in joystick mode
            }
            Err(broadcast::error::TryRecvError::Empty) => return false,
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                debug!("Event receiver lagged by {} events (drain)", n);
            }
            Err(broadcast::error::TryRecvError::Closed) => return true,
        }
    }
}

/// Apply coalesced depth events to the mapper, converting raw → mm.
fn apply_depths(pending_depths: &HashMap<u8, u16>, precision_factor: f64, mapper: &mut AxisMapper) {
    for (&key_index, &depth_raw) in pending_depths {
        let depth_mm = (depth_raw as f64 / precision_factor) as f32;
        mapper.update_key_depth(key_index, depth_mm);
    }
}

/// Run in headless mode (no TUI) with event-driven depth.
async fn run_headless(config: JoystickConfig, _config_path: PathBuf) -> Result<()> {
    info!("Running in headless mode");

    // Create virtual joystick
    let enabled_axes: Vec<_> = config
        .axes
        .iter()
        .filter(|a| a.enabled)
        .map(|a| a.id)
        .collect();

    let mut joystick = VirtualJoystick::new(&config.device_name, &enabled_axes)?;
    info!("Created virtual joystick: {}", config.device_name);

    if let Some(path) = joystick.device_path() {
        info!("Device path: {}", path.display());
    }

    let mut mapper = AxisMapper::new();

    info!("Entering main loop. Press Ctrl+C to exit.");

    loop {
        // Connect (or reconnect) to keyboard
        let conn = loop {
            if let Some(conn) = connect_keyboard().await {
                break conn;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        };

        let mut event_rx = conn.event_rx;
        let precision_factor = conn.precision_factor;

        // Event loop: recv + coalescing drain
        loop {
            match event_rx.recv().await {
                Ok(ts) => {
                    let mut pending_depths: HashMap<u8, u16> = HashMap::new();

                    // Process first event
                    if let VendorEvent::KeyDepth {
                        key_index,
                        depth_raw,
                    } = ts.event
                    {
                        pending_depths.insert(key_index, depth_raw);
                    }

                    // Drain remaining pending events (coalesce by key)
                    let closed = drain_depth_events(&mut event_rx, &mut pending_depths);

                    // Apply to mapper and update joystick
                    apply_depths(&pending_depths, precision_factor, &mut mapper);
                    let axis_values = mapper.compute_axes(&config);
                    if let Err(e) = joystick.set_axes(&axis_values) {
                        warn!("Failed to update joystick: {}", e);
                    }

                    if closed {
                        info!("Event channel closed, reconnecting...");
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    debug!("Event receiver lagged by {} events", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("Event channel closed, reconnecting...");
                    break;
                }
            }
        }

        // Stop magnetism before reconnect
        let _ = conn.keyboard.stop_magnetism_report().await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Run with TUI using event-driven depth from broadcast channel.
async fn run_tui(config: JoystickConfig, config_path: PathBuf) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    stdout().execute(EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new(config, config_path);

    // Create virtual joystick
    let mut joystick: Option<VirtualJoystick> = None;
    match create_joystick(&app.config) {
        Ok(js) => {
            info!("Created virtual joystick");
            app.joystick_status = JoystickStatus::Active;
            joystick = Some(js);
        }
        Err(e) => {
            error!("Failed to create joystick: {}", e);
            app.joystick_status = JoystickStatus::Error;
            app.status_message = Some(format!("Joystick error: {}", e));
        }
    }

    // Event subscription from keyboard (None until connected)
    let mut event_rx: Option<broadcast::Receiver<TimestampedEvent>> = None;

    // Spawn initial connection attempt
    app.keyboard_status = KeyboardStatus::Connecting;
    let mut reconnect_handle: Option<tokio::task::JoinHandle<Option<KeyboardConnection>>> =
        Some(tokio::spawn(async { connect_keyboard().await }));

    // Event stream for terminal input
    let mut events = EventStream::new();

    // Main loop
    let tick_rate = Duration::from_millis(50);
    let mut last_tick = std::time::Instant::now();

    loop {
        // Draw UI
        terminal.draw(|f| render::render(f, &app))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());

        tokio::select! {
            // Terminal events
            event = events.next() => {
                if let Some(Ok(event)) = event {
                    if handle_event(&mut app, event)? {
                        break;
                    }
                }
            }

            // Keyboard depth events - low-latency broadcast channel with coalescing
            result = async {
                match &mut event_rx {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                let mut pending_depths: HashMap<u8, u16> = HashMap::new();
                let mut channel_closed = false;

                match result {
                    Ok(ts) => {
                        if let VendorEvent::KeyDepth { key_index, depth_raw } = ts.event {
                            pending_depths.insert(key_index, depth_raw);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!("Event receiver lagged by {} events", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        channel_closed = true;
                    }
                }

                // Drain remaining events (coalesce depth by key)
                if !channel_closed {
                    if let Some(ref mut rx) = event_rx {
                        channel_closed = drain_depth_events(rx, &mut pending_depths);
                    }
                }

                // Apply coalesced depth events
                if !pending_depths.is_empty() {
                    apply_depths(&pending_depths, app.precision_factor, &mut app.mapper);

                    // Update pressed key display (show last key that moved)
                    if let Some(&key_index) = pending_depths.keys().next() {
                        app.pressed_key = Some(key_index);
                    }

                    // Update joystick
                    if let Some(ref mut js) = joystick {
                        let axis_values = app.mapper.compute_axes(&app.config);
                        if let Err(e) = js.set_axes(&axis_values) {
                            debug!("Failed to update joystick: {}", e);
                        }
                    }
                }

                if channel_closed {
                    event_rx = None;
                    app.keyboard_status = KeyboardStatus::Disconnected;
                    app.status_message = Some("Keyboard disconnected".to_string());
                    // Spawn reconnect
                    if reconnect_handle.is_none() {
                        reconnect_handle = Some(tokio::spawn(async {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            connect_keyboard().await
                        }));
                        app.keyboard_status = KeyboardStatus::Connecting;
                    }
                }
            }

            // Check if reconnection task completed
            result = async {
                match &mut reconnect_handle {
                    Some(h) => h.await,
                    None => std::future::pending().await,
                }
            } => {
                reconnect_handle = None;
                match result {
                    Ok(Some(conn)) => {
                        app.keyboard_status = KeyboardStatus::Connected;
                        app.precision_factor = conn.precision_factor;
                        app.status_message = Some(format!(
                            "Keyboard connected (precision factor: {:.0})",
                            conn.precision_factor
                        ));
                        event_rx = Some(conn.event_rx);
                        // conn.keyboard is dropped — that's fine, the transport
                        // and its reader thread continue running independently
                    }
                    Ok(None) => {
                        // Connection failed, retry
                        app.keyboard_status = KeyboardStatus::Disconnected;
                        reconnect_handle = Some(tokio::spawn(async {
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            connect_keyboard().await
                        }));
                        app.keyboard_status = KeyboardStatus::Connecting;
                    }
                    Err(e) => {
                        error!("Connect task panicked: {}", e);
                        app.keyboard_status = KeyboardStatus::Error;
                        app.status_message = Some("Connection task failed".to_string());
                    }
                }
            }

            // Tick timeout for UI refresh
            _ = tokio::time::sleep(timeout) => {
                // Periodic joystick update (for smooth display even without new events)
                if let Some(ref mut js) = joystick {
                    let axis_values = app.mapper.compute_axes(&app.config);
                    if let Err(e) = js.set_axes(&axis_values) {
                        debug!("Failed to update joystick: {}", e);
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = std::time::Instant::now();
        }

        if app.should_quit {
            break;
        }
    }

    // Cleanup
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    stdout().execute(DisableMouseCapture)?;

    Ok(())
}

/// Create virtual joystick from config
fn create_joystick(config: &JoystickConfig) -> Result<VirtualJoystick, anyhow::Error> {
    let enabled_axes: Vec<_> = config
        .axes
        .iter()
        .filter(|a| a.enabled)
        .map(|a| a.id)
        .collect();

    if enabled_axes.is_empty() {
        return Err(anyhow::anyhow!("No axes enabled"));
    }

    let joystick = VirtualJoystick::new(&config.device_name, &enabled_axes)?;
    Ok(joystick)
}

/// Handle terminal events
fn handle_event(app: &mut App, event: Event) -> Result<bool> {
    if let Event::Key(key) = event {
        if key.kind != KeyEventKind::Press {
            return Ok(false);
        }

        match key.code {
            KeyCode::Char('q') => {
                if app.config_dirty {
                    app.status_message =
                        Some("Unsaved changes! Press q again to quit.".to_string());
                    app.config_dirty = false; // Allow quit on second press
                } else {
                    return Ok(true);
                }
            }
            KeyCode::Char('?') => {
                app.show_help = !app.show_help;
            }
            KeyCode::Char('s') => {
                if let Err(e) = app.save_config() {
                    app.status_message = Some(format!("Save failed: {}", e));
                }
            }
            KeyCode::Tab | KeyCode::Char('1') => {
                app.set_mode(AppMode::Live);
            }
            KeyCode::Char('2') => {
                app.set_mode(AppMode::Configure);
            }
            KeyCode::Char('3') => {
                app.set_mode(AppMode::Calibrate);
            }
            KeyCode::Up => {
                app.select_prev();
            }
            KeyCode::Down => {
                app.select_next();
            }
            KeyCode::Enter => {
                app.select_enter();
            }
            KeyCode::Esc => {
                if app.show_help {
                    app.show_help = false;
                } else {
                    app.select_back();
                }
            }
            KeyCode::Char(' ') => {
                app.toggle_current();
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                app.adjust_current(1.0);
            }
            KeyCode::Char('-') => {
                app.adjust_current(-1.0);
            }
            _ => {}
        }
    }

    Ok(false)
}

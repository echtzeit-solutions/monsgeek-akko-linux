//! MonsGeek Magnetic Key-to-Joystick Mapper
//!
//! Main entry point and TUI run loop.

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use futures::StreamExt;
use ratatui::prelude::*;
use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use monsgeek_joystick::config::JoystickConfig;
use monsgeek_joystick::joystick::VirtualJoystick;
use monsgeek_joystick::mapper::AxisMapper;
use monsgeek_joystick::tui::app::{App, AppMode, JoystickStatus, KeyboardStatus};
use monsgeek_joystick::tui::render;

use monsgeek_keyboard::KeyboardInterface;
use monsgeek_transport::{list_devices_sync, open_device_sync};

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

/// Messages from the keyboard reader task
enum KeyboardMessage {
    Connected { precision: f64 },
    Disconnected,
    KeyDepth { key_index: u8, depth_mm: f32 },
    Error(String),
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

/// Run in headless mode (no TUI)
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

    // Connect to keyboard
    let devices = list_devices_sync()?;
    if devices.is_empty() {
        error!("No supported keyboard found");
        return Err(anyhow::anyhow!("No keyboard found"));
    }

    let transport = open_device_sync(&devices[0])?;
    let info = transport.device_info();
    info!(
        "Connected to keyboard: {} ({:04x}:{:04x})",
        info.product_name.as_deref().unwrap_or("Unknown"),
        info.vid,
        info.pid
    );

    // Get precision
    let keyboard = KeyboardInterface::new(
        transport.inner().clone(),
        monsgeek_keyboard::KEY_COUNT_M1_V5,
        true,
    );
    let precision = keyboard.get_precision().await?;
    let precision_factor = precision.factor();
    info!(
        "Precision: {} (factor: {})",
        precision.as_str(),
        precision_factor
    );

    // Start magnetism reporting
    keyboard.start_magnetism_report().await?;
    info!("Started magnetism reporting");

    // Main loop
    let mut mapper = AxisMapper::new();

    info!("Entering main loop. Press Ctrl+C to exit.");

    loop {
        match keyboard.read_key_depth(100, precision_factor).await {
            Ok(Some(event)) => {
                mapper.update_key_depth(event.key_index, event.depth_mm);

                // Compute and update axes
                let axis_values = mapper.compute_axes(&config);
                if let Err(e) = joystick.set_axes(&axis_values) {
                    warn!("Failed to update joystick: {}", e);
                }
            }
            Ok(None) => {
                // Timeout, continue
            }
            Err(e) => {
                error!("Error reading key depth: {}", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

/// Run with TUI
async fn run_tui(config: JoystickConfig, config_path: PathBuf) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    stdout().execute(EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new(config, config_path);

    // Channel for keyboard messages
    let (kb_tx, mut kb_rx) = mpsc::unbounded_channel::<KeyboardMessage>();

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

    // Spawn keyboard connection task
    let kb_tx_clone = kb_tx.clone();
    tokio::spawn(async move {
        keyboard_task(kb_tx_clone).await;
    });

    // Event stream
    let mut events = EventStream::new();

    // Main loop
    let tick_rate = Duration::from_millis(50);
    let mut last_tick = std::time::Instant::now();

    loop {
        // Draw UI
        terminal.draw(|f| render::render(f, &app))?;

        // Handle events with timeout
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

            // Keyboard messages
            msg = kb_rx.recv() => {
                if let Some(msg) = msg {
                    handle_keyboard_message(&mut app, &mut joystick, msg);
                }
            }

            // Tick timeout
            _ = tokio::time::sleep(timeout) => {
                // Update axes even without new events (for smooth display)
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

/// Handle keyboard messages
fn handle_keyboard_message(
    app: &mut App,
    joystick: &mut Option<VirtualJoystick>,
    msg: KeyboardMessage,
) {
    match msg {
        KeyboardMessage::Connected { precision } => {
            app.keyboard_status = KeyboardStatus::Connected;
            app.status_message = Some(format!("Keyboard connected (precision: {})", precision));
        }
        KeyboardMessage::Disconnected => {
            app.keyboard_status = KeyboardStatus::Disconnected;
            app.status_message = Some("Keyboard disconnected".to_string());
        }
        KeyboardMessage::KeyDepth {
            key_index,
            depth_mm,
        } => {
            app.update_key_depth(key_index, depth_mm);

            // Update joystick
            if let Some(ref mut js) = joystick {
                let axis_values = app.mapper.compute_axes(&app.config);
                if let Err(e) = js.set_axes(&axis_values) {
                    debug!("Failed to update joystick: {}", e);
                }
            }
        }
        KeyboardMessage::Error(e) => {
            app.keyboard_status = KeyboardStatus::Error;
            app.status_message = Some(format!("Keyboard error: {}", e));
        }
    }
}

/// Background task to connect to keyboard and read depth events
async fn keyboard_task(tx: mpsc::UnboundedSender<KeyboardMessage>) {
    loop {
        // Try to connect
        let devices = match list_devices_sync() {
            Ok(d) => d,
            Err(e) => {
                let _ = tx.send(KeyboardMessage::Error(format!("List devices: {}", e)));
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        if devices.is_empty() {
            let _ = tx.send(KeyboardMessage::Disconnected);
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }

        let transport = match open_device_sync(&devices[0]) {
            Ok(t) => t,
            Err(e) => {
                let _ = tx.send(KeyboardMessage::Error(format!("Open device: {}", e)));
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        let keyboard = KeyboardInterface::new(
            transport.inner().clone(),
            monsgeek_keyboard::KEY_COUNT_M1_V5,
            true,
        );

        // Get precision
        let precision_factor = match keyboard.get_precision().await {
            Ok(precision) => precision.factor(),
            Err(e) => {
                let _ = tx.send(KeyboardMessage::Error(format!("Get precision: {}", e)));
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        // Start magnetism reporting
        if let Err(e) = keyboard.start_magnetism_report().await {
            let _ = tx.send(KeyboardMessage::Error(format!("Start magnetism: {}", e)));
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }

        let _ = tx.send(KeyboardMessage::Connected {
            precision: precision_factor,
        });

        // Read loop
        loop {
            match keyboard.read_key_depth(100, precision_factor).await {
                Ok(Some(event)) => {
                    if tx
                        .send(KeyboardMessage::KeyDepth {
                            key_index: event.key_index,
                            depth_mm: event.depth_mm,
                        })
                        .is_err()
                    {
                        // Channel closed, exit
                        return;
                    }
                }
                Ok(None) => {
                    // Timeout, check if still connected
                    if !keyboard.is_connected().await {
                        let _ = tx.send(KeyboardMessage::Disconnected);
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(KeyboardMessage::Error(format!("Read depth: {}", e)));
                    break;
                }
            }
        }

        // Stop magnetism before reconnect attempt
        let _ = keyboard.stop_magnetism_report().await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

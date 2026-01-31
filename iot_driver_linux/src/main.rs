//! MonsGeek Keyboard Driver CLI
//!
//! A command-line interface for controlling MonsGeek keyboards.

use clap::Parser;
use hidapi::HidApi;
use tonic::transport::Server;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

// CLI definitions
mod cli;
use cli::{Cli, Commands, FirmwareCommands};

// Command handlers (split from main.rs)
mod commands;

// gRPC server module
mod grpc;
use grpc::{dj_dev, DriverGrpcServer, DriverService};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Handle --file flag for pcap replay mode (no device needed)
    if let Some(ref pcap_file) = cli.file {
        return iot_driver::pcap_analyzer::run_pcap_analysis(
            pcap_file,
            iot_driver::pcap_analyzer::OutputFormat::Text,
            cli.filter.as_deref(),
            false, // verbose
            false, // debug
            cli.hex,
            cli.all,
        );
    }

    // Create printer config if monitoring is enabled
    let printer_config =
        commands::create_printer_config(cli.monitor, cli.hex, cli.all, cli.filter.as_deref())?;

    match cli.command {
        None => {
            // Default: show device info
            commands::query::info(printer_config)?;
        }

        // === Query Commands ===
        Some(Commands::Info) => {
            commands::query::info(printer_config)?;
        }
        Some(Commands::Profile) => {
            commands::query::profile(printer_config)?;
        }
        Some(Commands::Led) => {
            commands::query::led(printer_config)?;
        }
        Some(Commands::Debounce) => {
            commands::query::debounce(printer_config)?;
        }
        Some(Commands::Rate) => {
            commands::query::rate()?;
        }
        Some(Commands::Options) => {
            commands::query::options(printer_config)?;
        }
        Some(Commands::Features) => {
            commands::query::features(printer_config)?;
        }
        Some(Commands::Sleep) => {
            commands::query::sleep()?;
        }
        Some(Commands::All) => {
            commands::query::all(printer_config)?;
        }
        Some(Commands::Battery {
            quiet,
            hex,
            watch,
            vendor,
        }) => {
            let hidapi = HidApi::new()?;
            commands::query::battery(&hidapi, quiet, hex, watch, vendor)?;
        }

        // === Set Commands ===
        Some(Commands::SetProfile { profile }) => {
            commands::set::set_profile(profile)?;
        }
        Some(Commands::SetDebounce { ms }) => {
            commands::set::set_debounce(ms)?;
        }
        Some(Commands::SetRate { rate }) => {
            commands::set::set_rate(&rate)?;
        }
        Some(Commands::SetLed {
            mode,
            brightness,
            speed,
            r,
            g,
            b,
        }) => {
            commands::set::set_led(&mode, brightness, speed, r, g, b)?;
        }
        Some(Commands::SetSleep {
            idle,
            deep,
            idle_bt,
            idle_24g,
            deep_bt,
            deep_24g,
            uniform,
        }) => {
            commands::set::set_sleep(idle, deep, idle_bt, idle_24g, deep_bt, deep_24g, uniform)?;
        }
        Some(Commands::Reset) => {
            commands::set::reset()?;
        }
        Some(Commands::SetColorAll { r, g, b, layer }) => {
            commands::set::set_color_all(r, g, b, layer)?;
        }

        // === Trigger Commands ===
        Some(Commands::Calibrate) => {
            commands::triggers::calibrate()?;
        }
        Some(Commands::Triggers) => {
            commands::triggers::triggers()?;
        }
        Some(Commands::SetActuation { mm }) => {
            commands::triggers::set_actuation(mm)?;
        }
        Some(Commands::SetRt { value }) => {
            commands::triggers::set_rt(&value)?;
        }
        Some(Commands::SetRelease { mm }) => {
            commands::triggers::set_release(mm)?;
        }
        Some(Commands::SetBottomDeadzone { mm }) => {
            commands::triggers::set_bottom_deadzone(mm)?;
        }
        Some(Commands::SetTopDeadzone { mm }) => {
            commands::triggers::set_top_deadzone(mm)?;
        }
        Some(Commands::SetKeyTrigger {
            key,
            actuation,
            release,
            mode,
        }) => {
            commands::triggers::set_key_trigger(key, actuation, release, mode)?;
        }

        // === Keymap Commands ===
        Some(Commands::Remap { from, to, layer }) => {
            commands::keymap::remap(&from, &to, layer)?;
        }
        Some(Commands::ResetKey { key, layer }) => {
            commands::keymap::reset_key(&key, layer)?;
        }
        Some(Commands::Swap { key1, key2, layer }) => {
            commands::keymap::swap(&key1, &key2, layer)?;
        }
        Some(Commands::Keymatrix { layer }) => {
            commands::keymap::keymatrix(layer)?;
        }

        // === Macro Commands ===
        Some(Commands::Macro { key }) => {
            commands::macros::get_macro(&key)?;
        }
        Some(Commands::SetMacro { key, text }) => {
            commands::macros::set_macro(&key, &text)?;
        }
        Some(Commands::ClearMacro { key }) => {
            commands::macros::clear_macro(&key)?;
        }
        Some(Commands::AssignMacro { key, macro_index }) => {
            commands::macros::assign_macro(&key, &macro_index)?;
        }

        // === Animation Commands ===
        Some(Commands::Gif {
            file,
            mode,
            test,
            frames,
            delay,
        }) => {
            commands::animations::gif(file.as_deref(), mode.into(), test, frames, delay)?;
        }
        Some(Commands::GifStream { file, mode, r#loop }) => {
            commands::animations::gif_stream(&file, mode.into(), r#loop)?;
        }
        Some(Commands::Mode { mode, layer }) => {
            commands::animations::mode(&mode, layer)?;
        }
        Some(Commands::Modes) => {
            commands::animations::modes()?;
        }

        // === Audio/Reactive Commands ===
        Some(Commands::Audio {
            mode,
            hue,
            sensitivity,
        }) => {
            commands::reactive::audio(mode.as_str(), hue, sensitivity)?;
        }
        Some(Commands::AudioTest) => {
            commands::reactive::audio_test()?;
        }
        Some(Commands::AudioLevels) => {
            commands::reactive::audio_levels()?;
        }
        #[cfg(feature = "screen-capture")]
        Some(Commands::Screen { fps }) => {
            commands::reactive::screen(fps).await?;
        }

        // === Debug Commands ===
        Some(Commands::Depth { raw, zero, verbose }) => {
            commands::debug::depth(raw, zero, verbose)?;
        }
        Some(Commands::TestTransport) => {
            commands::debug::test_transport()?;
        }

        // === Firmware Commands ===
        Some(Commands::Firmware(fw_cmd)) => match fw_cmd {
            FirmwareCommands::Info => {
                commands::firmware::info()?;
            }
            FirmwareCommands::Validate { file } => {
                commands::firmware::validate(&file)?;
            }
            FirmwareCommands::DryRun { file, verbose } => {
                commands::firmware::dry_run(&file, verbose)?;
            }
            FirmwareCommands::Check { device_id } => {
                commands::firmware::check(device_id).await?;
            }
            FirmwareCommands::Download { device_id, output } => {
                commands::firmware::download(device_id, &output).await?;
            }
        },

        // === Utility Commands ===
        Some(Commands::List) => {
            let hidapi = HidApi::new()?;
            commands::utility::list(&hidapi)?;
        }
        Some(Commands::Raw { cmd: cmd_str }) => {
            commands::utility::raw(&cmd_str, printer_config)?;
        }
        Some(Commands::Serve) => {
            run_server(printer_config).await?;
        }
        Some(Commands::Tui) => {
            commands::utility::tui().await?;
        }
        Some(Commands::Joystick { config, headless }) => {
            commands::utility::joystick(config, headless)?;
        }
    }

    Ok(())
}

async fn run_server(
    printer_config: Option<monsgeek_transport::PrinterConfig>,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("iot_driver=debug".parse().unwrap()),
        )
        .init();

    let addr = "127.0.0.1:3814".parse()?;

    info!("Starting IOT Driver Linux on {}", addr);
    if printer_config.is_some() {
        info!("Monitor mode enabled - printing all commands/responses");
    }
    println!("addr :: {addr}");

    let service = DriverService::with_printer_config(printer_config)
        .map_err(|e| format!("Failed to initialize HID API: {e}"))?;

    // Start hot-plug monitoring for device connect/disconnect
    service.start_hotplug_monitor();

    // Scan for devices on startup
    let devices = service.scan_devices().await;
    info!("Found {} devices on startup", devices.len());
    for dev in &devices {
        if let Some(dj_dev::OneofDev::Dev(d)) = &dev.oneof_dev {
            info!(
                "  - VID={:04x} PID={:04x} ID={} path={}",
                d.vid, d.pid, d.id, d.path
            );
        }
    }

    // CORS layer for browser access
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any)
        .expose_headers(Any);

    // Wrap service with gRPC-Web support for browser clients
    let grpc_service = tonic_web::enable(DriverGrpcServer::new(service));

    info!("Server ready with gRPC-Web support");

    Server::builder()
        .accept_http1(true)
        .tcp_nodelay(true)
        .initial_stream_window_size(4096)
        .initial_connection_window_size(4096)
        .layer(cors)
        .add_service(grpc_service)
        .serve(addr)
        .await?;

    Ok(())
}

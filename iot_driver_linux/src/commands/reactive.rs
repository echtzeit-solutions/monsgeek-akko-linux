//! Reactive mode command handlers (audio, screen).

use super::{setup_interrupt_handler, CommandResult};
use monsgeek_keyboard::KeyboardInterface;

/// Run audio reactive LED mode
pub fn audio(color_mode: &str, hue: f32, sensitivity: f32) -> CommandResult {
    let keyboard =
        KeyboardInterface::open_any().map_err(|e| format!("Failed to open device: {e}"))?;

    println!(
        "Starting audio reactive mode on {}...",
        keyboard.device_name()
    );
    println!("Press Ctrl+C to stop");

    let running = setup_interrupt_handler();

    let config = iot_driver::audio_reactive::AudioConfig {
        color_mode: color_mode.to_string(),
        base_hue: hue,
        sensitivity,
        smoothing: 0.3,
    };

    if let Err(e) = iot_driver::audio_reactive::run_audio_reactive(&keyboard, config, running) {
        eprintln!("Audio reactive error: {e}");
    }
    Ok(())
}

/// Test audio capture (list devices)
pub fn audio_test() -> CommandResult {
    println!("Testing audio capture...\n");

    println!("Available audio devices:");
    for name in iot_driver::audio_reactive::list_audio_devices() {
        println!("  - {name}");
    }
    println!();

    if let Err(e) = iot_driver::audio_reactive::test_audio_capture() {
        eprintln!("Audio test failed: {e}");
    }
    Ok(())
}

/// Show real-time audio levels
pub fn audio_levels() -> CommandResult {
    if let Err(e) = iot_driver::audio_reactive::test_audio_levels() {
        eprintln!("Audio levels test failed: {e}");
    }
    Ok(())
}

/// Run screen color reactive LED mode
#[cfg(feature = "screen-capture")]
pub async fn screen(fps: u32) -> CommandResult {
    let fps = fps.clamp(1, 60);

    let keyboard =
        KeyboardInterface::open_any().map_err(|e| format!("Failed to open device: {e}"))?;

    println!(
        "Starting screen color mode on {}...",
        keyboard.device_name()
    );
    println!("Press Ctrl+C to stop");

    let running = setup_interrupt_handler();

    if let Err(e) = iot_driver::screen_capture::run_screen_color_mode(&keyboard, running, fps).await
    {
        eprintln!("Screen color mode error: {e}");
    }
    Ok(())
}

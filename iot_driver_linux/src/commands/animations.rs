//! Animation command handlers.

use super::CommandResult;
use iot_driver::gif::{generate_test_animation, load_gif, print_animation_info, MappingMode};
use iot_driver::protocol::cmd::LedMode;
use monsgeek_keyboard::SyncKeyboard;

/// Upload GIF animation to keyboard flash (persistent, mode 25)
pub fn gif(
    file: Option<&str>,
    mode: MappingMode,
    test: bool,
    frames: usize,
    delay: u16,
) -> CommandResult {
    let keyboard = SyncKeyboard::open_any().map_err(|e| format!("Failed to open device: {e}"))?;

    let animation = if test {
        println!("Generating {frames} frame test animation...");
        generate_test_animation(frames, delay)
    } else if let Some(path) = file {
        println!("Loading GIF: {path}");
        match load_gif(path, mode) {
            Ok(anim) => anim,
            Err(e) => {
                eprintln!("Failed to load GIF: {e}");
                return Ok(());
            }
        }
    } else {
        eprintln!("Either provide a file path or use --test");
        return Ok(());
    };

    print_animation_info(&animation);

    let anim_frames: Vec<Vec<(u8, u8, u8)>> = animation
        .frames
        .iter()
        .take(255)
        .map(|f| f.colors.clone())
        .collect();

    let delay_ms = animation.frames.first().map(|f| f.delay_ms).unwrap_or(100);

    println!(
        "\nUploading {} frames ({}ms delay)...",
        anim_frames.len(),
        delay_ms
    );
    match keyboard.upload_animation(&anim_frames, delay_ms) {
        Ok(()) => println!("Animation uploaded! Keyboard will play it autonomously."),
        Err(e) => eprintln!("Failed to upload animation: {e}"),
    }
    Ok(())
}

/// Set LED mode by name or number
pub fn mode(keyboard: &SyncKeyboard, mode: &str, layer: u8) -> CommandResult {
    let led_mode = match LedMode::parse(mode) {
        Some(m) => m,
        None => {
            eprintln!("Unknown mode: {mode}");
            eprintln!("\nAvailable modes:");
            for (id, name) in LedMode::list_all() {
                eprintln!("  {id:2} - {name}");
            }
            return Ok(());
        }
    };

    println!(
        "Setting LED mode to {} ({}) with layer {}...",
        led_mode.name(),
        led_mode.as_u8(),
        layer
    );
    match keyboard.set_led_with_option(led_mode.as_u8(), 4, 0, 128, 128, 128, false, layer) {
        Ok(_) => println!("Done."),
        Err(e) => eprintln!("Failed to set LED mode: {e}"),
    }
    Ok(())
}

/// List all available LED modes
pub fn modes() -> CommandResult {
    println!("Available LED modes:");
    for (id, name) in LedMode::list_all() {
        println!("  {id:2} - {name}");
    }
    Ok(())
}

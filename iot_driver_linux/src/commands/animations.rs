//! Animation command handlers.

use super::{setup_interrupt_handler, CommandResult};
use iot_driver::gif::{generate_test_animation, load_gif, print_animation_info, MappingMode};
use iot_driver::protocol::cmd::LedMode;
use monsgeek_keyboard::SyncKeyboard;
use std::sync::atomic::Ordering;

/// Upload GIF animation to keyboard memory
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

/// Stream GIF animation in real-time
pub fn gif_stream(file: &str, mode: MappingMode, loop_anim: bool) -> CommandResult {
    println!("Loading GIF: {file}");
    let animation = load_gif(file, mode).map_err(|e| format!("Failed to load GIF: {e}"))?;

    print_animation_info(&animation);

    let keyboard = SyncKeyboard::open_any().map_err(|e| format!("Failed to open device: {e}"))?;

    let _ = keyboard.set_led_with_option(13, 4, 0, 0, 0, 0, false, 0);

    let running = setup_interrupt_handler();

    println!("\nStreaming animation (Ctrl+C to stop)...");

    loop {
        for (idx, frame) in animation.frames.iter().enumerate() {
            if !running.load(Ordering::SeqCst) {
                break;
            }

            let _ = keyboard.set_per_key_colors_fast(&frame.colors, 10, 3);
            print!("\rFrame {:3}/{}", idx + 1, animation.frame_count);
            std::io::Write::flush(&mut std::io::stdout()).ok();

            std::thread::sleep(std::time::Duration::from_millis(frame.delay_ms as u64));
        }

        if !loop_anim || !running.load(Ordering::SeqCst) {
            break;
        }
    }

    println!("\nAnimation stopped.");
    Ok(())
}

/// Set LED mode by name or number
pub fn mode(mode: &str, layer: u8) -> CommandResult {
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

    match SyncKeyboard::open_any() {
        Ok(keyboard) => {
            println!(
                "Setting LED mode to {} ({}) with layer {}...",
                led_mode.name(),
                led_mode.as_u8(),
                layer
            );
            match keyboard.set_led_with_option(led_mode.as_u8(), 4, 0, 128, 128, 128, false, layer)
            {
                Ok(_) => println!("Done."),
                Err(e) => eprintln!("Failed to set LED mode: {e}"),
            }
        }
        Err(e) => eprintln!("Failed to open device: {e}"),
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

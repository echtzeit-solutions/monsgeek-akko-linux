//! Animation command handlers.

use super::CommandResult;
use iot_driver::protocol::cmd::LedMode;
use monsgeek_keyboard::KeyboardInterface;

/// Set LED mode by name or number
pub fn mode(keyboard: &KeyboardInterface, mode: &str, layer: u8) -> CommandResult {
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

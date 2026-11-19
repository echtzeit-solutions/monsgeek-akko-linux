//! Key remapping command handlers.

use super::CommandResult;
use iot_driver::key_action::KeyAction;
use iot_driver::keymap::{self, KeyRef, Layer};
use iot_driver::keymatrix_view::{self, ListOptions};
use iot_driver::protocol::hid;
use monsgeek_keyboard::RyKeyboard;
use monsgeek_transport::protocol::{KeymatrixLayer, Profile};

/// Remap a key.
///
/// `from` can include a layer prefix: `"Fn+Caps"`, `"L1+A"`, `"42"`.
/// When a layer prefix is present, it takes precedence over the `--layer` flag.
pub fn remap(keyboard: &RyKeyboard, from: &str, to: &str, layer: Layer) -> CommandResult {
    let key_ref: KeyRef = match from.parse() {
        Ok(kr) => kr,
        Err(msg) => {
            eprintln!("{msg}");
            return Ok(());
        }
    };

    // If from has a layer prefix (not Base when the raw string contains "+"),
    // use that; otherwise use the --layer flag as override.
    let effective_layer = if from.contains('+') {
        key_ref.layer
    } else {
        layer
    };

    let action: KeyAction = match to.parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Invalid target key: {e}");
            return Ok(());
        }
    };

    let display_ref = KeyRef::new(key_ref.index, effective_layer);
    println!(
        "Remapping {} (index {}) to {action} on {} layer...",
        display_ref,
        key_ref.index.get(),
        effective_layer.name()
    );
    match keymap::set_key(keyboard, key_ref.index, effective_layer, &action) {
        Ok(()) => println!("{display_ref} remapped to {action}"),
        Err(e) => eprintln!("Failed to remap key: {e}"),
    }
    Ok(())
}

/// Reset a key to default.
///
/// `key` can include a layer prefix: `"Fn+Caps"`, `"L1+A"`.
pub fn reset_key(keyboard: &RyKeyboard, key: &str, layer: Layer) -> CommandResult {
    let key_ref: KeyRef = match key.parse() {
        Ok(kr) => kr,
        Err(msg) => {
            eprintln!("{msg}");
            return Ok(());
        }
    };

    let effective_layer = if key.contains('+') {
        key_ref.layer
    } else {
        layer
    };

    let display_ref = KeyRef::new(key_ref.index, effective_layer);
    println!(
        "Resetting {} (index {}) on {}...",
        display_ref,
        key_ref.index.get(),
        effective_layer.name()
    );
    match keymap::reset_key(keyboard, key_ref.index, effective_layer) {
        Ok(()) => println!("{display_ref} reset to default"),
        Err(e) => eprintln!("Failed to reset key: {e}"),
    }
    Ok(())
}

/// Swap two keys within a profile.
///
/// Only ever touches keymatrix layer 0: `swap_keys` writes through
/// `set_keymatrix(profile, .., layer: 0)`, so the read must match.
pub fn swap(keyboard: &RyKeyboard, key1: &str, key2: &str, profile: Profile) -> CommandResult {
    let kr_a: KeyRef = match key1.parse() {
        Ok(kr) => kr,
        Err(msg) => {
            eprintln!("{msg}");
            return Ok(());
        }
    };
    let kr_b: KeyRef = match key2.parse() {
        Ok(kr) => kr,
        Err(msg) => {
            eprintln!("{msg}");
            return Ok(());
        }
    };

    match keyboard.get_keymatrix(profile, KeymatrixLayer::BASE, 8) {
        Ok(data) => {
            let (key_a, key_b) = (kr_a.index, kr_b.index);
            // The keymatrix entry's usage sits at byte 2 of the 4-byte slot.
            let usage_at = |pos: monsgeek_transport::protocol::MatrixPos| {
                data.get(usize::from(pos) * 4 + 2)
                    .copied()
                    .map(monsgeek_transport::protocol::HidUsage::new)
                    .unwrap_or(monsgeek_transport::protocol::HidUsage::NONE)
            };
            let (code_a, code_b) = (usage_at(key_a), usage_at(key_b));

            let name_a = keyboard.matrix_key_name(key_a.into());
            let name_b = keyboard.matrix_key_name(key_b.into());
            let action_a = hid::key_name(code_a);
            let action_b = hid::key_name(code_b);
            println!("Swapping {name_a} ({action_a}) <-> {name_b} ({action_b})...");

            match keyboard.swap_keys(
                profile,
                key_a.get(),
                code_a.get(),
                key_b.get(),
                code_b.get(),
            ) {
                Ok(()) => println!("Keys swapped successfully"),
                Err(e) => eprintln!("Failed to swap keys: {e}"),
            }
        }
        Err(e) => eprintln!("Failed to read current key mappings: {e}"),
    }
    Ok(())
}

/// Show per-key bindings across layers.
pub fn keymatrix(
    keyboard: &RyKeyboard,
    layers: &[crate::cli::LayerArg],
    unset: bool,
    keys: &[iot_driver::keyclass::KeySelector],
    sys: crate::cli::SysArg,
    raw: bool,
) -> CommandResult {
    let rows = match keymap::load_key_rows(keyboard, sys.wire(), &progress_to_stderr) {
        Ok(rows) => rows,
        Err(e) => {
            clear_progress_line();
            eprintln!("Failed to read key matrix: {e}");
            return Ok(());
        }
    };
    clear_progress_line();
    // An empty selector list means "every key", which is what resolve() returns —
    // but pass it through as empty so the renderer skips the per-key filter.
    let selected = if keys.is_empty() {
        Vec::new()
    } else {
        iot_driver::keyclass::KeySelector::resolve(keys)
    };
    let opts = ListOptions {
        layers: layers.iter().map(|&l| l.into()).collect(),
        keys: selected,
        show_unset: unset,
        raw,
        precision: keyboard.get_precision().unwrap_or_default(),
    };
    print!("{}", keymatrix_view::render(&rows, &opts));
    Ok(())
}

/// Progress line for the paged reads behind a full key-matrix load.
///
/// Reading every table is ~70 round trips, and on BLE each carries a 150 ms settle
/// gap — long enough to look hung. Stays on one line via `\r` and only when stderr
/// is a terminal, so piped or redirected output is unchanged.
fn progress_to_stderr(stage: keymap::LoadStage) {
    use std::io::{IsTerminal, Write};
    let mut err = std::io::stderr();
    if !err.is_terminal() {
        return;
    }
    // done + 1, matching the TUI gauge: the named stage is in flight, not finished.
    let width = 20;
    let filled = (stage.done + 1) * width / stage.total;
    let _ = write!(
        err,
        "\r\x1b[2Kreading {stage} [{}{}]",
        "#".repeat(filled),
        "·".repeat(width - filled),
    );
    let _ = err.flush();
}

/// Clear the progress line once the load is done.
fn clear_progress_line() {
    use std::io::{IsTerminal, Write};
    let mut err = std::io::stderr();
    if err.is_terminal() {
        let _ = write!(err, "\r\x1b[2K");
        let _ = err.flush();
    }
}

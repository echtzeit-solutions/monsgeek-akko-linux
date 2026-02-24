//! CLI command handlers for the effect engine.

use std::collections::BTreeMap;

use super::CommandResult;
use iot_driver::effect::{self, EffectLibrary};
use iot_driver::notify::keymap;

/// Parse `--var key=value` arguments into a BTreeMap.
pub fn parse_vars(var_args: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut vars = BTreeMap::new();
    for arg in var_args {
        let (k, v) = arg
            .split_once('=')
            .ok_or_else(|| format!("invalid --var format: '{arg}' (expected key=value)"))?;
        vars.insert(k.to_string(), v.to_string());
    }
    Ok(vars)
}

/// List all effects.
pub fn list() -> CommandResult {
    let lib = EffectLibrary::load_default().map_err(|e| format!("load effects: {e}"))?;

    println!(
        "Effects (from {}):",
        effect::default_effects_path().display()
    );
    println!();
    println!(
        "{:<16} {:<8} {:<6} {:<10} Description",
        "Name", "KFs", "Prio", "TTL"
    );
    println!("{}", "-".repeat(60));

    for (name, def) in &lib.effects {
        let kfs = def.keyframes.len();
        let ttl = match def.ttl_ms {
            Some(ms) if ms > 0 => format!("{}ms", ms),
            _ => "-".to_string(),
        };
        let desc = def.description.as_deref().unwrap_or("");
        let vars = effect::required_variables(def);
        let var_str = if vars.is_empty() {
            String::new()
        } else {
            format!(" [vars: {}]", vars.join(", "))
        };
        println!(
            "{:<16} {:<8} {:<6} {:<10} {}{}",
            name, kfs, def.priority, ttl, desc, var_str
        );
    }

    Ok(())
}

/// Show details of a single effect.
pub fn show(name: &str) -> CommandResult {
    let lib = EffectLibrary::load_default().map_err(|e| format!("load effects: {e}"))?;

    let def = lib
        .get(name)
        .ok_or_else(|| format!("unknown effect: {name}"))?;

    println!("Effect: {name}");
    if let Some(ref desc) = def.description {
        println!("Description: {desc}");
    }
    if let Some(ref color) = def.color {
        println!("Color: {color}");
    }
    if let Some(ref mode) = def.mode {
        println!("Mode: {mode}");
    }
    if let Some(speed) = def.speed {
        println!("Speed: {speed}");
    }
    println!("Priority: {}", def.priority);
    if let Some(ttl) = def.ttl_ms {
        println!("TTL: {}ms", ttl);
    }

    let vars = effect::required_variables(def);
    if !vars.is_empty() {
        println!("Required variables: {}", vars.join(", "));
    }

    if def.keyframes.is_empty() {
        println!("\nNo keyframes (solid effect)");
    } else {
        println!("\nKeyframes:");
        println!("  {:<8} {:<8} {:<12} Color", "Time", "Value", "Easing");
        println!("  {}", "-".repeat(44));
        let duration = def.keyframes.last().map(|kf| kf.t).unwrap_or(0.0);
        for kf in &def.keyframes {
            let color = kf.color.as_deref().unwrap_or("-");
            println!("  {:<8.0} {:<8.2} {:<12} {}", kf.t, kf.v, kf.easing, color);
        }
        println!("\nDuration: {duration:.0}ms");
    }

    Ok(())
}

/// Run terminal preview.
pub fn preview(name: &str, keys: &[String], var_args: &[String], fps: u32) -> CommandResult {
    let lib = EffectLibrary::load_default().map_err(|e| format!("load effects: {e}"))?;
    let vars = parse_vars(var_args)?;

    let def = lib
        .get(name)
        .ok_or_else(|| format!("unknown effect: {name}"))?;

    // Resolve keys to matrix indices
    let mut indices = Vec::new();
    for key in keys {
        let idx = keymap::parse_key_target(key)?;
        indices.extend(idx);
    }

    if indices.is_empty() {
        // Default: show on F1-F4
        for key in &["F1", "F2", "F3", "F4"] {
            if let Ok(idx) = keymap::parse_key_target(key) {
                indices.extend(idx);
            }
        }
    }

    effect::preview::run(def, &indices, &vars, fps)?;
    Ok(())
}

/// Play an effect on hardware.
pub fn play(
    printer_config: Option<monsgeek_transport::PrinterConfig>,
    name: &str,
    keys: &[String],
    var_args: &[String],
) -> CommandResult {
    let lib = EffectLibrary::load_default().map_err(|e| format!("load effects: {e}"))?;
    let vars = parse_vars(var_args)?;

    let def = lib
        .get(name)
        .ok_or_else(|| format!("unknown effect: {name}"))?;

    let resolved = effect::resolve(def, &vars).map_err(|e| {
        let required = effect::required_variables(def);
        format!("{e} (required variables: {})", required.join(", "))
    })?;

    // Resolve keys
    let mut indices = Vec::new();
    for key in keys {
        let idx = keymap::parse_key_target(key)?;
        indices.extend(idx);
    }

    if indices.is_empty() {
        return Err("specify at least one key".into());
    }

    let kb = super::led_stream::open_with_patch_check(printer_config)?;
    let running = super::setup_interrupt_handler();

    println!(
        "Playing '{}' on {} key(s) (Ctrl+C to stop)",
        name,
        indices.len()
    );

    effect::preview::play_on_hardware(&kb, &resolved, &indices, &running)?;

    println!("\nReleasing LED stream...");
    kb.stream_led_release().ok();
    println!("Done.");
    Ok(())
}

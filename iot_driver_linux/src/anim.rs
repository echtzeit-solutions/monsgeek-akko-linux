//! On-device animation engine abstraction.
//!
//! Provides a high-level interface over the firmware's 0xEA animation commands.
//! Used by CLI (`anim-status`), TUI (periodic status display), and the
//! notification daemon (programming effects).

use std::fmt;
use std::sync::Arc;

use monsgeek_keyboard::{AnimDefStatus, AnimStatus, KeyboardInterface};

use crate::effect::{self, CompiledAnim, ResolvedEffect};

/// Firmware animation tick rate in Hz (~100 Hz blend rate).
pub const TICK_RATE_HZ: f64 = 100.0;
/// Milliseconds per firmware animation tick.
pub const MS_PER_TICK: f64 = 1000.0 / TICK_RATE_HZ;

/// Cached info about what was programmed into a firmware def slot.
#[derive(Debug, Clone)]
pub struct SlotEntry {
    /// Effect template name (e.g. "breathe", "flash").
    pub effect_name: String,
    /// The resolved effect, ready to evaluate for sparkline rendering.
    pub resolved: crate::effect::ResolvedEffect,
    /// The compiled wire format — used for identity matching (same effect+vars = same compiled).
    pub compiled: CompiledAnim,
}

/// Per-slot metadata shared between daemon/TUI preview and the renderer.
#[derive(Debug, Clone, Default)]
pub struct SlotInfo {
    slots: [Option<SlotEntry>; 8],
}

impl SlotInfo {
    pub fn set(&mut self, def_id: u8, entry: SlotEntry) {
        if (def_id as usize) < 8 {
            self.slots[def_id as usize] = Some(entry);
        }
    }

    pub fn clear(&mut self, def_id: u8) {
        if (def_id as usize) < 8 {
            self.slots[def_id as usize] = None;
        }
    }

    pub fn clear_all(&mut self) {
        self.slots = Default::default();
    }

    pub fn get(&self, def_id: u8) -> Option<&SlotEntry> {
        self.slots.get(def_id as usize).and_then(|o| o.as_ref())
    }
}

/// Thread-safe shared slot info.
pub type SharedSlotInfo = Arc<std::sync::Mutex<SlotInfo>>;

/// High-level animation engine handle.
///
/// Wraps the keyboard interface with convenience methods for programming
/// and querying animations. Shared across CLI, TUI, and daemon.
///
/// Accepts either `Arc<KeyboardInterface>` (for TUI/daemon) or constructs
/// from a reference (for CLI one-shot commands).
#[derive(Clone)]
pub struct AnimEngine {
    kb: Arc<KeyboardInterface>,
}

/// Key assignment: strip index + phase offset.
#[derive(Debug, Clone, Copy)]
pub struct KeyAssignment {
    pub strip_idx: u8,
    pub phase_offset: u8,
}

/// Snapshot of the firmware animation engine state.
#[derive(Debug, Clone)]
pub struct EngineSnapshot {
    pub raw: AnimStatus,
    /// Per-def key assignments (def_id → list). Populated by `query_full()`.
    pub keys: std::collections::HashMap<u8, Vec<KeyAssignment>>,
}

impl EngineSnapshot {
    /// Frame count (monotonic, wraps at u32::MAX). Useful for synchronization.
    pub fn frame_count(&self) -> u32 {
        self.raw.frame_count
    }

    /// Number of active animation definitions.
    pub fn active_count(&self) -> u8 {
        self.raw.active_count
    }

    /// Whether the LED overlay is currently active.
    pub fn overlay_active(&self) -> bool {
        self.raw.overlay_active
    }

    /// Active animation definition slots.
    pub fn defs(&self) -> &[AnimDefStatus] {
        &self.raw.defs
    }

    /// Duration of one animation cycle in milliseconds.
    pub fn def_duration_ms(def: &AnimDefStatus) -> f64 {
        def.duration_ticks as f64 * MS_PER_TICK
    }

    /// Current phase position (0.0-1.0) within the animation cycle,
    /// derived from the global frame_count. Each key's actual phase
    /// is offset by its phase_offset.
    pub fn def_phase(&self, def: &AnimDefStatus) -> f64 {
        if def.duration_ticks == 0 {
            return 0.0;
        }
        let elapsed = self.raw.frame_count % def.duration_ticks as u32;
        elapsed as f64 / def.duration_ticks as f64
    }
}

impl fmt::Display for EngineSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} active", self.raw.active_count)?;
        for d in &self.raw.defs {
            let mode = match (d.is_one_shot(), d.is_rainbow()) {
                (true, true) => "one-shot+rainbow",
                (true, false) => "one-shot",
                (false, true) => "rainbow",
                (false, false) => "loop",
            };
            write!(
                f,
                "\n  def[{}]: {}KF pri={} {}keys {}",
                d.id, d.num_kf, d.priority, d.key_count, mode,
            )?;
        }
        Ok(())
    }
}

/// A programmed animation on the device, tracking the slot it occupies.
#[derive(Debug, Clone)]
pub struct ProgrammedAnim {
    pub def_id: u8,
    pub compiled: CompiledAnim,
    pub key_count: usize,
}

impl AnimEngine {
    pub fn new(kb: Arc<KeyboardInterface>) -> Self {
        Self { kb }
    }

    /// Access the underlying keyboard interface.
    pub fn kb(&self) -> &KeyboardInterface {
        &self.kb
    }

    /// Check if the firmware supports the animation engine.
    pub fn is_available(&self) -> bool {
        self.kb
            .get_patch_info()
            .ok()
            .flatten()
            .is_some_and(|p| p.has_anim_engine())
    }

    /// Query the current animation engine state (status only, no key lists).
    pub fn query(&self) -> Result<EngineSnapshot, String> {
        self.kb
            .anim_query()
            .map_err(|e| format!("anim_query: {e}"))?
            .map(|raw| EngineSnapshot {
                raw,
                keys: std::collections::HashMap::new(),
            })
            .ok_or_else(|| "animation engine not available".to_string())
    }

    /// Query status + key assignments for all active defs.
    ///
    /// Does N+1 USB round-trips (status + one per active def with keys).
    /// Spaces queries 10ms apart to avoid flooding the firmware.
    pub fn query_full(&self) -> Result<EngineSnapshot, String> {
        let mut snap = self.query()?;
        for d in &snap.raw.defs {
            if d.key_count > 0 {
                std::thread::sleep(std::time::Duration::from_millis(10));
                if let Ok(pairs) = self.kb.anim_query_keys(d.id) {
                    snap.keys.insert(
                        d.id,
                        pairs
                            .into_iter()
                            .map(|(s, p)| KeyAssignment {
                                strip_idx: s,
                                phase_offset: p,
                            })
                            .collect(),
                    );
                }
            }
        }
        Ok(snap)
    }

    /// Program an effect into a specific definition slot.
    ///
    /// Returns the compiled animation for reference.
    pub fn program(
        &self,
        def_id: u8,
        effect: &ResolvedEffect,
        priority: i8,
        one_shot: bool,
        keys: &[(u8, u8)], // (matrix_idx, phase_offset)
    ) -> Result<ProgrammedAnim, String> {
        let compiled = effect
            .compile_for_firmware(priority, one_shot)
            .ok_or("effect has no keyframes")?;

        self.kb
            .anim_define(
                def_id,
                compiled.flags,
                compiled.priority,
                compiled.duration_ticks,
                &compiled.keyframes,
            )
            .map_err(|e| format!("anim_define: {e}"))?;

        self.kb
            .anim_assign(def_id, keys)
            .map_err(|e| format!("anim_assign: {e}"))?;

        Ok(ProgrammedAnim {
            def_id,
            compiled,
            key_count: keys.len(),
        })
    }

    /// Program a pre-compiled animation.
    pub fn program_compiled(
        &self,
        def_id: u8,
        compiled: &CompiledAnim,
        keys: &[(u8, u8)],
    ) -> Result<(), String> {
        self.kb
            .anim_define(
                def_id,
                compiled.flags,
                compiled.priority,
                compiled.duration_ticks,
                &compiled.keyframes,
            )
            .map_err(|e| format!("anim_define: {e}"))?;

        self.kb
            .anim_assign(def_id, keys)
            .map_err(|e| format!("anim_assign: {e}"))?;

        Ok(())
    }

    /// Cancel a specific animation slot.
    pub fn cancel(&self, def_id: u8) -> Result<(), String> {
        self.kb
            .anim_cancel(def_id)
            .map_err(|e| format!("anim_cancel: {e}"))
    }

    /// Clear all animations and overlay.
    pub fn clear(&self) -> Result<(), String> {
        self.kb.anim_clear().map_err(|e| format!("anim_clear: {e}"))
    }
}

/// Query animation status directly from a keyboard reference (for CLI one-shots).
pub fn query_status(kb: &KeyboardInterface) -> Result<EngineSnapshot, String> {
    kb.anim_query()
        .map_err(|e| format!("anim_query: {e}"))?
        .map(|raw| EngineSnapshot {
            raw,
            keys: std::collections::HashMap::new(),
        })
        .ok_or_else(|| "animation engine not available".to_string())
}

/// Convert a stagger delay in ms to a firmware phase_offset value.
///
/// Each phase_offset unit = 8 ticks at the firmware blend rate.
/// At ~120Hz: 1 unit ≈ 66.7ms.
pub fn ms_to_phase_offset(ms: f64) -> u8 {
    let ticks = ms / MS_PER_TICK;
    (ticks / 8.0).round().clamp(0.0, 255.0) as u8
}

/// Resolve an effect by name from the library with variables, then compile.
pub fn compile_effect(
    lib: &effect::EffectLibrary,
    name: &str,
    vars: &std::collections::BTreeMap<String, String>,
    priority: i8,
    one_shot: bool,
) -> Result<CompiledAnim, String> {
    let def = lib
        .get(name)
        .ok_or_else(|| format!("unknown effect: {name}"))?;
    let resolved = effect::resolve(def, vars)?;
    resolved
        .compile_for_firmware(priority, one_shot)
        .ok_or_else(|| "effect has no keyframes".to_string())
}

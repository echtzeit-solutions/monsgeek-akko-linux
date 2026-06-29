//! Native PulseAudio capture + source enumeration.
//!
//! Replaces the previous cpal/ALSA path. Works transparently on PipeWire via its
//! pulse server (pipewire-pulse). Requires the libpulse client dev package at
//! build time (e.g. `libpulse-dev` on Debian/Ubuntu).

use std::cell::RefCell;
use std::rc::Rc;

use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::{Context, FlagSet, State as ContextState};
use libpulse_binding::def::BufferAttr;
use libpulse_binding::mainloop::standard::{IterateResult, Mainloop};
use libpulse_binding::operation::{Operation, State as OpState};
use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::Direction;
use libpulse_simple_binding::Simple;

/// Fixed capture rate; the FFT path assumes mono f32 at this rate.
pub const SAMPLE_RATE: u32 = 44100;

const APP_NAME: &str = "iot_driver";
const STREAM_NAME: &str = "audio-reactive";

/// A capture source (input device or sink monitor).
#[derive(Clone, Debug)]
pub struct SourceEntry {
    pub name: String,
    pub description: String,
    pub is_monitor: bool,
}

impl SourceEntry {
    /// Human label: description, a `[monitor]` tag, and the raw source name.
    pub fn label(&self) -> String {
        let tag = if self.is_monitor { " [monitor]" } else { "" };
        format!("{}{} ({})", self.description, tag, self.name)
    }
}

/// Connect a context, run `action`, then disconnect.
fn with_context<T>(
    action: impl FnOnce(&mut Mainloop, &Context) -> Result<T, String>,
) -> Result<T, String> {
    let mut mainloop = Mainloop::new().ok_or("Failed to create PulseAudio mainloop")?;
    let mut context =
        Context::new(&mainloop, APP_NAME).ok_or("Failed to create PulseAudio context")?;
    context
        .connect(None, FlagSet::NOFLAGS, None)
        .map_err(|e| format!("PulseAudio connect failed: {e}"))?;

    loop {
        pump(&mut mainloop)?;
        match context.get_state() {
            ContextState::Ready => break,
            ContextState::Failed => return Err("PulseAudio context failed".into()),
            ContextState::Terminated => return Err("PulseAudio context terminated".into()),
            _ => {}
        }
    }

    let result = action(&mut mainloop, &context);
    context.disconnect();
    result
}

/// Run one blocking mainloop iteration, mapping failure to an error.
fn pump(mainloop: &mut Mainloop) -> Result<(), String> {
    match mainloop.iterate(true) {
        IterateResult::Success(_) => Ok(()),
        IterateResult::Quit(_) => Err("PulseAudio mainloop quit".into()),
        IterateResult::Err(e) => Err(format!("PulseAudio mainloop error: {e}")),
    }
}

/// Pump the mainloop until `op` finishes.
fn wait_for_op<T: ?Sized>(mainloop: &mut Mainloop, op: &Operation<T>) -> Result<(), String> {
    loop {
        pump(mainloop)?;
        match op.get_state() {
            OpState::Done => return Ok(()),
            OpState::Cancelled => return Err("PulseAudio operation cancelled".into()),
            OpState::Running => {}
        }
    }
}

/// List all capture sources (inputs + sink monitors).
pub fn list_sources() -> Result<Vec<SourceEntry>, String> {
    with_context(|mainloop, context| {
        let sources = Rc::new(RefCell::new(Vec::new()));
        let collect = sources.clone();
        let op = context.introspect().get_source_info_list(move |res| {
            if let ListResult::Item(info) = res {
                collect.borrow_mut().push(SourceEntry {
                    name: info.name.as_deref().unwrap_or_default().to_string(),
                    description: info.description.as_deref().unwrap_or_default().to_string(),
                    is_monitor: info.monitor_of_sink.is_some(),
                });
            }
        });
        wait_for_op(mainloop, &op)?;
        let collected = sources.borrow().clone();
        Ok(collected)
    })
}

/// The default sink's monitor source name (`<sink>.monitor`), if discoverable.
pub fn default_monitor_name() -> Option<String> {
    with_context(|mainloop, context| {
        let name = Rc::new(RefCell::new(None));
        let collect = name.clone();
        let op = context.introspect().get_server_info(move |info| {
            *collect.borrow_mut() = info.default_sink_name.as_deref().map(str::to_string);
        });
        wait_for_op(mainloop, &op)?;
        let resolved = name.borrow().clone();
        Ok(resolved)
    })
    .ok()
    .flatten()
    .map(|sink| format!("{sink}.monitor"))
}

/// Resolve which source to capture from.
///
/// With `requested`: exact match on name or description, else case-insensitive
/// substring; errors list candidates on no/ambiguous match. Without `requested`:
/// the default sink's monitor, else the first monitor, else the first source.
pub fn resolve_source(requested: Option<&str>) -> Result<SourceEntry, String> {
    let sources = list_sources()?;
    if sources.is_empty() {
        return Err("No PulseAudio capture sources found".into());
    }

    let Some(req) = requested else {
        if let Some(def) = default_monitor_name() {
            if let Some(s) = sources.iter().find(|s| s.name == def) {
                return Ok(s.clone());
            }
        }
        if let Some(s) = sources.iter().find(|s| s.is_monitor) {
            return Ok(s.clone());
        }
        return Ok(sources.into_iter().next().unwrap());
    };

    if let Some(s) = sources
        .iter()
        .find(|s| s.name == req || s.description == req)
    {
        return Ok(s.clone());
    }
    let req_lower = req.to_lowercase();
    let matches: Vec<&SourceEntry> = sources
        .iter()
        .filter(|s| {
            s.name.to_lowercase().contains(&req_lower)
                || s.description.to_lowercase().contains(&req_lower)
        })
        .collect();
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!(
            "No capture source matches '{req}'. Available sources:\n  - {}",
            label_list(&sources)
        )),
        many => Err(format!(
            "'{req}' is ambiguous, matches {} sources:\n  - {}",
            many.len(),
            many.iter()
                .map(|s| s.label())
                .collect::<Vec<_>>()
                .join("\n  - ")
        )),
    }
}

fn label_list(sources: &[SourceEntry]) -> String {
    sources
        .iter()
        .map(SourceEntry::label)
        .collect::<Vec<_>>()
        .join("\n  - ")
}

/// Open a blocking record stream (mono f32 @ [`SAMPLE_RATE`]) on `source`.
pub fn open_record(source: &str) -> Result<Simple, String> {
    let spec = Spec {
        format: Format::F32le,
        channels: 1,
        rate: SAMPLE_RATE,
    };
    if !spec.is_valid() {
        return Err("Invalid PulseAudio sample spec".into());
    }
    // Request small fragments so the monitor advances smoothly. The server
    // default can be very large (~hundreds of ms), which makes the captured
    // audio — and thus the visualizer — only advance a few times per second.
    // ~10ms of mono f32.
    let frag = (SAMPLE_RATE / 100) * 4;
    let attr = BufferAttr {
        maxlength: frag * 4,
        tlength: u32::MAX,
        prebuf: u32::MAX,
        minreq: u32::MAX,
        fragsize: frag,
    };
    Simple::new(
        None,     // default server
        APP_NAME, // application name
        Direction::Record,
        Some(source), // source name
        STREAM_NAME,  // stream description
        &spec,
        None,        // default channel map
        Some(&attr), // small fragments for low-latency capture
    )
    .map_err(|e| format!("Failed to open PulseAudio source '{source}': {e}"))
}

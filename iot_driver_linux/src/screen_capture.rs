// Screen Color Reactive LED Mode
// Captures average screen color via PipeWire ScreenCast and streams to keyboard

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::hid::MonsGeekDevice;
use crate::protocol::{cmd, screen_color};

/// Screen color state shared between capture and main loop
pub struct ScreenColorState {
    /// Current average RGB color
    pub color: Mutex<(u8, u8, u8)>,
    /// Running flag
    pub running: AtomicBool,
}

impl Default for ScreenColorState {
    fn default() -> Self {
        Self {
            color: Mutex::new((0, 0, 0)),
            running: AtomicBool::new(false),
        }
    }
}

impl ScreenColorState {
    pub fn get_color(&self) -> (u8, u8, u8) {
        *self.color.lock().unwrap()
    }

    pub fn set_color(&self, r: u8, g: u8, b: u8) {
        *self.color.lock().unwrap() = (r, g, b);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// Compute average RGB color from raw pixel data
/// Assumes BGRA or RGBA format (4 bytes per pixel)
pub fn compute_average_color(data: &[u8], width: u32, height: u32, is_bgra: bool) -> (u8, u8, u8) {
    let pixel_count = (width * height) as u64;
    if pixel_count == 0 || data.len() < (pixel_count * 4) as usize {
        return (0, 0, 0);
    }

    let (mut r_sum, mut g_sum, mut b_sum) = (0u64, 0u64, 0u64);

    for pixel in data.chunks_exact(4) {
        if is_bgra {
            // BGRA format (common on Linux)
            b_sum += pixel[0] as u64;
            g_sum += pixel[1] as u64;
            r_sum += pixel[2] as u64;
        } else {
            // RGBA format
            r_sum += pixel[0] as u64;
            g_sum += pixel[1] as u64;
            b_sum += pixel[2] as u64;
        }
    }

    (
        (r_sum / pixel_count) as u8,
        (g_sum / pixel_count) as u8,
        (b_sum / pixel_count) as u8,
    )
}

/// Run the screen color streaming loop
/// Sends RGB to keyboard at ~50Hz
fn run_screen_color_loop(
    device: &MonsGeekDevice,
    state: &Arc<ScreenColorState>,
    running: Arc<AtomicBool>,
) -> Result<(), String> {
    let update_interval = Duration::from_millis(screen_color::UPDATE_INTERVAL_MS);
    let mut last_color = (0u8, 0u8, 0u8);

    running.store(true, Ordering::SeqCst);
    state.running.store(true, Ordering::SeqCst);

    while running.load(Ordering::SeqCst) && state.is_running() {
        let frame_start = Instant::now();

        let (r, g, b) = state.get_color();

        // Only send if color changed (reduces USB traffic)
        if (r, g, b) != last_color {
            // Debug: print color
            println!("Screen color: RGB({}, {}, {}) #{:02X}{:02X}{:02X}", r, g, b, r, g, b);

            let report = screen_color::build_report(r, g, b);
            // Use the standard send method with Bit7 checksum
            device.send(cmd::SET_SCREEN_COLOR, &report[1..8], crate::protocol::ChecksumType::Bit7);
            last_color = (r, g, b);
        }

        let elapsed = frame_start.elapsed();
        if elapsed < update_interval {
            std::thread::sleep(update_interval - elapsed);
        }
    }

    Ok(())
}

/// Screen capture via PipeWire ScreenCast portal
pub mod pipewire_capture {
    use super::*;
    use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
    use ashpd::desktop::PersistMode;

    /// Start screen capture using XDG ScreenCast portal + PipeWire
    pub async fn start_capture(state: Arc<ScreenColorState>, fps: u32) -> Result<(), String> {
        // 1. Request screen cast via XDG portal
        let screencast = Screencast::new()
            .await
            .map_err(|e| format!("Failed to create screencast portal: {}", e))?;

        let session = screencast
            .create_session()
            .await
            .map_err(|e| format!("Failed to create session: {}", e))?;

        // Select monitor source
        screencast
            .select_sources(
                &session,
                CursorMode::Hidden,
                SourceType::Monitor.into(),
                false, // single source
                None,  // restore token
                PersistMode::DoNot,
            )
            .await
            .map_err(|e| format!("Failed to select sources: {}", e))?;

        // Start the stream - pass None for window identifier
        let response = screencast
            .start(&session, None)
            .await
            .map_err(|e| format!("Failed to start screencast: {}", e))?;

        let response = response
            .response()
            .map_err(|e| format!("Failed to get response: {}", e))?;

        let streams = response.streams();
        if streams.is_empty() {
            return Err("No streams returned from screencast".to_string());
        }

        let node_id = streams[0].pipe_wire_node_id();
        println!("Got PipeWire node ID: {}", node_id);

        // 2. Connect to PipeWire and receive frames
        // This runs in a blocking thread since pipewire-rs uses its own event loop
        let state_clone = state.clone();
        std::thread::spawn(move || {
            if let Err(e) = run_pipewire_capture(node_id, state_clone, fps) {
                eprintln!("PipeWire capture error: {}", e);
            }
        });

        Ok(())
    }

    /// Run PipeWire capture loop (blocking)
    fn run_pipewire_capture(node_id: u32, state: Arc<ScreenColorState>, fps: u32) -> Result<(), String> {
        use pipewire as pw;
        use pipewire::context::Context;
        use pipewire::main_loop::MainLoop;
        use pipewire::stream::{Stream, StreamFlags};
        use pipewire::spa::utils::Direction;
        use std::cell::RefCell;
        use std::rc::Rc;

        // Initialize PipeWire
        pipewire::init();

        let main_loop = MainLoop::new(None)
            .map_err(|e| format!("Failed to create main loop: {:?}", e))?;
        let context = Context::new(&main_loop)
            .map_err(|e| format!("Failed to create context: {:?}", e))?;
        let core = context
            .connect(None)
            .map_err(|e| format!("Failed to connect to PipeWire: {:?}", e))?;

        // Create properties for the stream
        let props = pipewire::properties::properties! {
            *pipewire::keys::MEDIA_TYPE => "Video",
            *pipewire::keys::MEDIA_CATEGORY => "Capture",
            *pipewire::keys::MEDIA_ROLE => "Screen",
        };

        // Create stream
        let stream = Stream::new(&core, "screen-color-capture", props)
            .map_err(|e| format!("Failed to create stream: {:?}", e))?;

        // State for video format
        let format_width: Rc<RefCell<u32>> = Rc::new(RefCell::new(0));
        let format_height: Rc<RefCell<u32>> = Rc::new(RefCell::new(0));
        let format_width_clone = format_width.clone();
        let format_height_clone = format_height.clone();
        let state_clone = state.clone();

        // Set up stream listener
        let _listener = stream
            .add_local_listener_with_user_data(())
            .state_changed(|_, _, _old, _new| {
                // Stream state changed (Connecting -> Paused -> Streaming)
            })
            .param_changed(move |_, _, id, pod| {
                use pipewire::spa::param::ParamType;
                if id != ParamType::Format.as_raw() {
                    return;
                }
                if let Some(pod) = pod {
                    if let Some((w, h)) = parse_video_format_size(pod) {
                        *format_width_clone.borrow_mut() = w;
                        *format_height_clone.borrow_mut() = h;
                    }
                }
            })
            .process(move |stream, _| {
                if let Some(mut buffer) = stream.dequeue_buffer() {
                    let width = *format_width.borrow();
                    let height = *format_height.borrow();

                    let datas = buffer.datas_mut();
                    if !datas.is_empty() {
                        if let Some(data) = datas[0].data() {
                            if width > 0 && height > 0 {
                                // Assume BGRx format (common)
                                let (r, g, b) = compute_average_color(data, width, height, true);
                                state_clone.set_color(r, g, b);
                            }
                        }
                    }
                }
            })
            .register()
            .map_err(|e| format!("Failed to register listener: {:?}", e))?;

        // Build video format parameters
        let mut params_buffer = vec![0u8; 1024];
        let obj = pw::spa::pod::object!(
            pw::spa::utils::SpaTypes::ObjectParamFormat,
            pw::spa::param::ParamType::EnumFormat,
            pw::spa::pod::property!(
                pw::spa::param::format::FormatProperties::MediaType,
                Id,
                pw::spa::param::format::MediaType::Video
            ),
            pw::spa::pod::property!(
                pw::spa::param::format::FormatProperties::MediaSubtype,
                Id,
                pw::spa::param::format::MediaSubtype::Raw
            ),
            pw::spa::pod::property!(
                pw::spa::param::format::FormatProperties::VideoFormat,
                Choice,
                Enum,
                Id,
                pw::spa::param::video::VideoFormat::BGRx,
                pw::spa::param::video::VideoFormat::BGRx,
                pw::spa::param::video::VideoFormat::RGBx,
                pw::spa::param::video::VideoFormat::RGBA,
                pw::spa::param::video::VideoFormat::BGRA
            ),
            pw::spa::pod::property!(
                pw::spa::param::format::FormatProperties::VideoSize,
                Choice,
                Range,
                Rectangle,
                pw::spa::utils::Rectangle { width: 320, height: 240 },
                pw::spa::utils::Rectangle { width: 1, height: 1 },
                pw::spa::utils::Rectangle { width: 4096, height: 4096 }
            ),
            pw::spa::pod::property!(
                pw::spa::param::format::FormatProperties::VideoFramerate,
                Choice,
                Range,
                Fraction,
                pw::spa::utils::Fraction { num: fps, denom: 1 },
                pw::spa::utils::Fraction { num: 0, denom: 1 },
                pw::spa::utils::Fraction { num: 60, denom: 1 }
            )
        );

        let pod = pw::spa::pod::serialize::PodSerializer::serialize(
            std::io::Cursor::new(&mut params_buffer),
            &pw::spa::pod::Value::Object(obj),
        )
        .map_err(|e| format!("Failed to serialize params: {:?}", e))?
        .0
        .into_inner();

        let pod = pw::spa::pod::Pod::from_bytes(&pod)
            .ok_or("Failed to create pod from bytes")?;

        // Connect to the screencast node
        stream
            .connect(
                Direction::Input,
                Some(node_id),
                StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
                &mut [pod],
            )
            .map_err(|e| format!("Failed to connect stream: {:?}", e))?;

        state.running.store(true, Ordering::SeqCst);

        // Get the loop reference for iteration
        let loop_ = main_loop.loop_();

        // Run main loop
        loop {
            // Check if we should stop
            if !state.is_running() {
                break;
            }

            // Process PipeWire events (this drives the stream callbacks)
            let n_events = loop_.iterate(Duration::from_millis(50));

            if n_events < 0 {
                eprintln!("PipeWire iterate() returned error: {}", n_events);
                break;
            }
        }
        Ok(())
    }

    /// Parse video size from SPA format pod
    fn parse_video_format_size(pod: &pipewire::spa::pod::Pod) -> Option<(u32, u32)> {
        use pipewire::spa::param::video::VideoInfoRaw;

        // Try to parse as VideoInfoRaw
        let mut info = VideoInfoRaw::new();
        if info.parse(pod).is_ok() {
            let size = info.size();
            if size.width > 0 && size.height > 0 {
                return Some((size.width, size.height));
            }
        }

        None
    }
}

/// Run screen color reactive mode (async entry point)
pub async fn run_screen_color_mode(
    device: &MonsGeekDevice,
    running: Arc<AtomicBool>,
    fps: u32,
) -> Result<(), String> {
    println!("Starting screen color mode ({}fps)...", fps);

    // Set LED mode to Screen Color (mode 21)
    device.set_led_with_option(cmd::LedMode::ScreenColor.as_u8(), 4, 4, 0, 0, 0, false, 0);
    std::thread::sleep(Duration::from_millis(200));

    // Create shared state
    let state = Arc::new(ScreenColorState::default());

    // Start PipeWire capture (requests permission via portal)
    println!("Requesting screen capture permission...");
    pipewire_capture::start_capture(state.clone(), fps).await?;

    // Give PipeWire time to start
    std::thread::sleep(Duration::from_millis(500));

    // Run the color streaming loop (blocking)
    println!("Streaming screen colors to keyboard...");
    run_screen_color_loop(device, &state, running)?;

    state.stop();
    println!("Screen color mode stopped");
    Ok(())
}

/// Synchronous wrapper for running screen color mode
pub fn run_screen_color_mode_sync(
    device: &MonsGeekDevice,
    running: Arc<AtomicBool>,
    fps: u32,
) -> Result<(), String> {
    // Create a new tokio runtime for the async parts
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("Failed to create runtime: {}", e))?;

    rt.block_on(run_screen_color_mode(device, running, fps))
}

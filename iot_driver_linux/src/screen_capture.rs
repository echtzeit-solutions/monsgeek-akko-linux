// Screen Color Reactive LED Mode
// Captures average screen color via PipeWire ScreenCast and streams to keyboard

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tracing::trace;

use crate::protocol::{cmd, screen_color};
use crate::screen_calib::{ColorCalibration, Region};
use crate::settings::Settings;
use monsgeek_keyboard::KeyboardInterface;

/// Screen color state shared between capture and main loop. Calibration/region/
/// test-swatch are live-adjustable (the loop and PipeWire callback read them each
/// frame), so TUI edits take effect without restarting the capture.
pub struct ScreenColorState {
    /// Latest raw average RGB color (pre-calibration).
    pub color: Mutex<(u8, u8, u8)>,
    /// Running flag
    pub running: AtomicBool,
    /// Color transform applied before streaming.
    calibration: RwLock<ColorCalibration>,
    /// Sub-rectangle of the screen that drives the average.
    region: RwLock<Region>,
    /// When set, stream this fixed color instead of the screen average (used to
    /// tune calibration against a known target).
    test_swatch: Mutex<Option<(u8, u8, u8)>>,
}

impl Default for ScreenColorState {
    fn default() -> Self {
        Self {
            color: Mutex::new((0, 0, 0)),
            running: AtomicBool::new(false),
            calibration: RwLock::new(ColorCalibration::default()),
            region: RwLock::new(Region::default()),
            test_swatch: Mutex::new(None),
        }
    }
}

impl ScreenColorState {
    /// New state seeded with the persisted calibration and region.
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            calibration: RwLock::new(settings.screen_calibration),
            region: RwLock::new(settings.screen_region),
            ..Self::default()
        }
    }

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

    pub fn calibration(&self) -> ColorCalibration {
        *self.calibration.read().unwrap()
    }

    pub fn set_calibration(&self, c: ColorCalibration) {
        *self.calibration.write().unwrap() = c;
    }

    pub fn region(&self) -> Region {
        *self.region.read().unwrap()
    }

    pub fn set_region(&self, r: Region) {
        *self.region.write().unwrap() = r;
    }

    pub fn test_swatch(&self) -> Option<(u8, u8, u8)> {
        *self.test_swatch.lock().unwrap()
    }

    pub fn set_test_swatch(&self, s: Option<(u8, u8, u8)>) {
        *self.test_swatch.lock().unwrap() = s;
    }
}

/// Grid size for sampling (16x16 = 256 samples instead of millions)
const SAMPLE_GRID_SIZE: u32 = 16;

/// Compute average RGB color by sampling a grid of pixels within `region`
/// (a normalized sub-rectangle; use `Region::default()` for the whole screen).
/// Much faster than averaging all pixels for high-res frames.
/// Assumes BGRA or RGBA format (4 bytes per pixel).
pub fn compute_average_color(
    data: &[u8],
    width: u32,
    height: u32,
    is_bgra: bool,
    region: Region,
) -> (u8, u8, u8) {
    if width == 0 || height == 0 || data.len() < (width * height * 4) as usize {
        return (0, 0, 0);
    }

    // Restrict sampling to the region's sub-rectangle.
    let region = region.sanitized();
    let x0 = (region.left * width as f32) as u32;
    let y0 = (region.top * height as f32) as u32;
    let x1 = ((region.right * width as f32) as u32).clamp(x0 + 1, width);
    let y1 = ((region.bottom * height as f32) as u32).clamp(y0 + 1, height);
    let (rw, rh) = (x1 - x0, y1 - y0);

    let stride = width * 4; // bytes per row
    let (mut r_sum, mut g_sum, mut b_sum) = (0u32, 0u32, 0u32);
    let mut sample_count = 0u32;

    // Sample a grid of points across the region
    for gy in 0..SAMPLE_GRID_SIZE {
        let y = (y0 + gy * rh / SAMPLE_GRID_SIZE) as usize;
        let row_offset = y * stride as usize;

        for gx in 0..SAMPLE_GRID_SIZE {
            let x = (x0 + gx * rw / SAMPLE_GRID_SIZE) as usize;
            let pixel_offset = row_offset + x * 4;

            if pixel_offset + 3 < data.len() {
                if is_bgra {
                    b_sum += data[pixel_offset] as u32;
                    g_sum += data[pixel_offset + 1] as u32;
                    r_sum += data[pixel_offset + 2] as u32;
                } else {
                    r_sum += data[pixel_offset] as u32;
                    g_sum += data[pixel_offset + 1] as u32;
                    b_sum += data[pixel_offset + 2] as u32;
                }
                sample_count += 1;
            }
        }
    }

    if sample_count == 0 {
        return (0, 0, 0);
    }

    (
        (r_sum / sample_count) as u8,
        (g_sum / sample_count) as u8,
        (b_sum / sample_count) as u8,
    )
}

/// Run the screen color streaming loop
/// Sends RGB to keyboard at ~50Hz.
///
/// `show_readout` draws the live in-place truecolor swatch on stdout (CLI only);
/// the TUI passes `false` to keep its alternate screen uncorrupted.
fn run_screen_color_loop(
    keyboard: &KeyboardInterface,
    state: &Arc<ScreenColorState>,
    running: Arc<AtomicBool>,
    show_readout: bool,
) -> Result<(), String> {
    let update_interval = Duration::from_millis(screen_color::UPDATE_INTERVAL_MS);
    let mut last_color = (0u8, 0u8, 0u8);

    // `running` is the authoritative stop signal (cleared by `signal_stop`); do
    // NOT force it back to `true` here or an in-flight stop would be lost. The
    // PipeWire thread owns `state.running` and sets it on connect.
    while running.load(Ordering::SeqCst) {
        let frame_start = Instant::now();

        // A test swatch (calibration helper) overrides the live average so the
        // user can tune against a known target; calibration is always applied.
        let raw = state.test_swatch().unwrap_or_else(|| state.get_color());
        let (r, g, b) = state.calibration().apply(raw);

        // Only send if color changed (reduces USB traffic)
        if (r, g, b) != last_color {
            trace!("Screen color: RGB({r}, {g}, {b}) #{r:02X}{g:02X}{b:02X}");

            if show_readout {
                // Live in-place readout of the calculated color being streamed:
                // truecolor swatch + hex + rgb.
                use std::io::Write;
                print!(
                    "\r  Screen color: \x1b[48;2;{r};{g};{b}m      \x1b[0m  #{r:02X}{g:02X}{b:02X}  rgb({r:>3},{g:>3},{b:>3})  "
                );
                let _ = std::io::stdout().flush();
            }

            let report = screen_color::build_report(r, g, b);
            // No-delay streaming send (the default flow-control delay would cap us).
            let _ = keyboard.send_raw_cmd_fast(cmd::SET_SCREEN_COLOR, &report[1..8]);
            last_color = (r, g, b);
        }

        let elapsed = frame_start.elapsed();
        if elapsed < update_interval {
            std::thread::sleep(update_interval - elapsed);
        }
    }

    if show_readout {
        println!(); // end the in-place readout line
    }
    Ok(())
}

/// Screen capture via PipeWire ScreenCast portal
pub mod pipewire_capture {
    use super::*;
    use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
    use ashpd::desktop::{PersistMode, Session};

    /// App identity reported to the XDG portal via the host Registry. KDE shows
    /// this in the "sharing contents to …" picker/tray (instead of blank), and
    /// the ScreenCast restore token is namespaced to it — so a stable id is what
    /// makes the saved token actually match on later runs. It must match the
    /// installed `<APP_ID>.desktop` (an independent project id — we are not the
    /// vendor), or the portal rejects it with "App info not found".
    const APP_ID: &str = "solutions.echtzeit.akko_keyboard_driver";

    /// Process-lifetime runtime that owns the ashpd/zbus D-Bus connection.
    ///
    /// ashpd caches its session-bus connection in a process-global `OnceLock`,
    /// and zbus (built with the `tokio` feature) pins that connection's socket
    /// reader task to whichever runtime is current when the connection is first
    /// created. If each capture used a fresh runtime and dropped it on teardown
    /// (as a per-thread `Runtime` does when the worker exits), the cached
    /// connection would die with the first runtime and every later ScreenSync
    /// entry would hang on the very first portal call. One never-dropped runtime
    /// keeps the connection alive across re-entry. (Proven by
    /// `examples/screencast_reentry_test.rs`.)
    pub fn portal_runtime() -> &'static tokio::runtime::Runtime {
        use std::sync::OnceLock;
        static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
        RT.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .thread_name("akko-portal")
                .build()
                .expect("build portal runtime")
        })
    }

    /// A live screencast session: held for the capture's lifetime so the portal
    /// stream stays valid.
    pub struct CaptureSession {
        _screencast: Screencast<'static>,
        /// Keeps the portal session (and thus the PipeWire stream) alive while
        /// capturing; closed via [`CaptureSession::shutdown`] on teardown.
        session: Session<'static, Screencast<'static>>,
        /// PipeWire frame loop — stopped via `state`, then joined on teardown.
        pipewire_thread: Option<std::thread::JoinHandle<()>>,
        /// Shared capture state; cleared to stop the PipeWire loop.
        state: Arc<ScreenColorState>,
    }

    /// Compositor-side teardown can lag behind dropping the session; a short
    /// pause before re-opening with the restore token avoids stale/black streams.
    pub const REENTRY_SETTLE_MS: u64 = 300;

    /// Upper bound on waiting for the portal `Close` reply. The Close request is
    /// sent on the first poll (stopping the recording); this only caps how long
    /// we wait for the acknowledgement before giving up.
    const CLOSE_TIMEOUT_MS: u64 = 1500;

    impl CaptureSession {
        /// Full teardown shared by the CLI and TUI capture paths: stop+join the
        /// PipeWire frame thread, then issue a bounded portal `Close` so the
        /// compositor actually stops the screencast (otherwise the tray
        /// "recording" indicator stays on and the session leaks, breaking
        /// re-entry — merely dropping the handle does NOT close it, since ashpd
        /// shares one cached D-Bus connection).
        ///
        /// MUST be driven on the **same runtime that created the ashpd/zbus
        /// connection**. The `Close` request is sent on the first poll (so the
        /// recording stops even if the reply is slow); the timeout only caps how
        /// long we wait for that reply, so this can never hang.
        pub async fn shutdown(mut self) {
            self.state.stop();
            if let Some(h) = self.pipewire_thread.take() {
                let _ = h.join();
            }
            let close = async {
                if let Err(e) = self.session.close().await {
                    tracing::debug!("screencast session close: {e}");
                }
            };
            let _ = tokio::time::timeout(Duration::from_millis(CLOSE_TIMEOUT_MS), close).await;
        }
    }

    /// Tell the portal who we are, so the picker/tray show a name and the restore
    /// token is keyed to a stable app id. Best-effort: older portals lack the
    /// host Registry interface, in which case this is a harmless no-op.
    async fn register_app_id(screencast: &Screencast<'_>) {
        use std::collections::HashMap;

        use ashpd::zbus::zvariant::Value;

        // `screencast` derefs (ashpd Proxy → zbus Proxy) to the shared connection
        // ashpd uses for the actual screencast request, which is exactly the
        // connection the portal must associate the app id with.
        let opts: HashMap<&str, Value> = HashMap::new();
        let res = screencast
            .connection()
            .call_method(
                Some("org.freedesktop.portal.Desktop"),
                "/org/freedesktop/portal/desktop",
                Some("org.freedesktop.host.portal.Registry"),
                "Register",
                &(APP_ID, opts),
            )
            .await;
        if let Err(e) = res {
            tracing::debug!("portal app-id Register unavailable: {e}");
        }
    }

    /// Start screen capture using XDG ScreenCast portal + PipeWire.
    ///
    /// Reuses a previously persisted restore token (and requests
    /// [`PersistMode::ExplicitlyRevoked`]) so the portal only prompts the first
    /// time, even across app restarts; the token returned by the compositor is
    /// saved back to `settings.toml`. (`PersistMode::Application` would be
    /// dropped by the compositor when the process exits, re-prompting next run.)
    ///
    /// Returns the [`CaptureSession`] — keep it alive while capturing and
    /// `close()` it when done.
    pub async fn start_capture(
        state: Arc<ScreenColorState>,
        fps: u32,
    ) -> Result<CaptureSession, String> {
        // 1. Request screen cast via XDG portal
        let screencast: Screencast<'static> = Screencast::new()
            .await
            .map_err(|e| format!("Failed to create screencast portal: {e}"))?;

        register_app_id(&screencast).await;

        let session = screencast
            .create_session()
            .await
            .map_err(|e| format!("Failed to create session: {e}"))?;

        // Reuse a saved restore token to skip the picker on subsequent runs.
        let saved_token = Settings::load().screencast_restore_token;

        // Select monitor source
        screencast
            .select_sources(
                &session,
                CursorMode::Hidden,
                SourceType::Monitor.into(),
                false,                  // single source
                saved_token.as_deref(), // restore token (if any)
                PersistMode::ExplicitlyRevoked,
            )
            .await
            .map_err(|e| format!("Failed to select sources: {e}"))?;

        // Start the stream - pass None for window identifier
        let response = screencast
            .start(&session, None)
            .await
            .map_err(|e| format!("Failed to start screencast: {e}"))?;

        let response = response
            .response()
            .map_err(|e| format!("Failed to get response: {e}"))?;

        // Persist the (possibly new) restore token for next time.
        if let Some(token) = response.restore_token() {
            if Some(token) != saved_token.as_deref() {
                Settings::update(|s| s.screencast_restore_token = Some(token.to_string()));
            }
        }

        let streams = response.streams();
        if streams.is_empty() {
            return Err("No streams returned from screencast".to_string());
        }

        let node_id = streams[0].pipe_wire_node_id();
        trace!("Got PipeWire node ID: {node_id}");

        // 2. Connect to PipeWire and receive frames (blocking thread).
        let state_clone = state.clone();
        let pipewire_thread = std::thread::spawn(move || {
            if let Err(e) = run_pipewire_capture(node_id, state_clone, fps) {
                tracing::error!("PipeWire capture error: {e}");
            }
        });

        Ok(CaptureSession {
            _screencast: screencast,
            session,
            pipewire_thread: Some(pipewire_thread),
            state,
        })
    }

    /// Run PipeWire capture loop (blocking)
    fn run_pipewire_capture(
        node_id: u32,
        state: Arc<ScreenColorState>,
        fps: u32,
    ) -> Result<(), String> {
        use pipewire as pw;
        use pipewire::context::ContextBox;
        use pipewire::main_loop::MainLoopBox;
        use pipewire::spa::utils::Direction;
        use pipewire::stream::{StreamBox, StreamFlags};
        use std::cell::RefCell;
        use std::rc::Rc;

        // Initialize PipeWire (MainLoopBox::new does this automatically)
        let main_loop =
            MainLoopBox::new(None).map_err(|e| format!("Failed to create main loop: {e:?}"))?;
        let context = ContextBox::new(main_loop.loop_(), None)
            .map_err(|e| format!("Failed to create context: {e:?}"))?;
        let core = context
            .connect(None)
            .map_err(|e| format!("Failed to connect to PipeWire: {e:?}"))?;

        // Create properties for the stream
        let props = pipewire::properties::properties! {
            *pipewire::keys::MEDIA_TYPE => "Video",
            *pipewire::keys::MEDIA_CATEGORY => "Capture",
            *pipewire::keys::MEDIA_ROLE => "Screen",
        };

        // Create stream
        let stream = StreamBox::new(&core, "screen-color-capture", props)
            .map_err(|e| format!("Failed to create stream: {e:?}"))?;

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
                                let (r, g, b) = compute_average_color(
                                    data,
                                    width,
                                    height,
                                    true,
                                    state_clone.region(),
                                );
                                state_clone.set_color(r, g, b);
                            }
                        }
                    }
                }
            })
            .register()
            .map_err(|e| format!("Failed to register listener: {e:?}"))?;

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
                pw::spa::utils::Rectangle {
                    width: 320,
                    height: 240
                },
                pw::spa::utils::Rectangle {
                    width: 1,
                    height: 1
                },
                pw::spa::utils::Rectangle {
                    width: 4096,
                    height: 4096
                }
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
        .map_err(|e| format!("Failed to serialize params: {e:?}"))?
        .0
        .into_inner();

        let pod = pw::spa::pod::Pod::from_bytes(pod).ok_or("Failed to create pod from bytes")?;

        // Connect to the screencast node
        stream
            .connect(
                Direction::Input,
                Some(node_id),
                StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
                &mut [pod],
            )
            .map_err(|e| format!("Failed to connect stream: {e:?}"))?;

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
                tracing::error!("PipeWire iterate() returned error: {n_events}");
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
    keyboard: &KeyboardInterface,
    running: Arc<AtomicBool>,
    fps: u32,
) -> Result<(), String> {
    println!("Starting screen color mode ({fps}fps)...");

    // Snapshot the current LED config so the previous mode + brightness/speed/
    // color can be restored on exit. Enter ScreenSync using those same values
    // (not placeholders), so the stored config is never overwritten mid-session.
    let saved = keyboard.get_led_params().ok();
    let (br, sp, r, g, b, dz) = match &saved {
        Some(p) => (
            p.brightness,
            p.speed,
            p.color.r,
            p.color.g,
            p.color.b,
            (p.direction & 0x0F) == monsgeek_keyboard::led::DAZZLE_ON,
        ),
        None => (4, 0, 0, 0, 0, false),
    };
    let _ = keyboard.set_led_with_option(cmd::LedMode::ScreenSync.as_u8(), br, sp, r, g, b, dz, 0);
    std::thread::sleep(Duration::from_millis(200));

    // Create shared state
    let state = Arc::new(ScreenColorState::from_settings(&Settings::load()));

    // Start PipeWire capture (requests permission via portal)
    println!("Requesting screen capture permission...");
    let capture = pipewire_capture::start_capture(state.clone(), fps).await?;

    // Give PipeWire time to start
    std::thread::sleep(Duration::from_millis(500));

    // Run the color streaming loop (blocking)
    println!("Streaming screen colors to keyboard...");
    let result = run_screen_color_loop(keyboard, &state, running, true);

    capture.shutdown().await;

    // Return to the previously selected LED mode with the user's settings intact.
    if let Some(p) = saved {
        let _ = keyboard.set_led_params(&p);
    }
    println!("Screen color mode stopped");
    result
}

/// Handle returned by [`spawn_for_tui`] — shared state plus the worker thread.
pub struct TuiScreenCapture {
    pub(crate) state: Arc<ScreenColorState>,
    /// Cleared by [`signal_stop`] to stop the keyboard streaming loop.
    running: Arc<AtomicBool>,
    /// Populated if portal/PipeWire setup fails inside the worker thread.
    pub(crate) error: Arc<Mutex<Option<String>>>,
    thread: std::thread::JoinHandle<()>,
}

impl TuiScreenCapture {
    /// True when the worker thread has exited (success or failure).
    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }

    /// Request the worker to stop (non-blocking).
    pub fn signal_stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.state.stop();
    }

    /// Block until the worker exits. Only call from a blocking thread, and only
    /// when [`is_finished`] is true unless shutting down the whole process.
    pub fn join(self) {
        let _ = self.thread.join();
    }
}

/// Start screen-reactive mode for the TUI: spawn a background thread that runs
/// the portal negotiation + PipeWire capture + the keyboard streaming loop, all
/// silently (no stdout). The LED mode is assumed to already be ScreenSync
/// (set by the caller).
pub fn spawn_for_tui(keyboard: Arc<KeyboardInterface>, fps: u32) -> TuiScreenCapture {
    let state = Arc::new(ScreenColorState::from_settings(&Settings::load()));
    let running = Arc::new(AtomicBool::new(true));
    let error = Arc::new(Mutex::new(None));

    let thread_state = state.clone();
    let thread_running = running.clone();
    let thread_error = error.clone();
    let thread = std::thread::spawn(move || {
        // Drive the async portal negotiation + teardown on the shared, process-
        // lifetime portal runtime — NOT a per-capture runtime. The latter would
        // be dropped when this worker exits, killing ashpd's cached D-Bus
        // connection and making the next ScreenSync entry hang (see
        // `portal_runtime`). The blocking streaming loop runs directly on this
        // std thread, off the runtime's workers.
        let rt = pipewire_capture::portal_runtime();

        // Portal negotiation only needs `block_on`; the returned session is held
        // (its background tasks run on the runtime's worker threads) so the
        // PipeWire stream stays valid during streaming.
        let capture = match rt.block_on(pipewire_capture::start_capture(thread_state.clone(), fps))
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("screen capture: {e}");
                *thread_error.lock().unwrap() = Some(e);
                thread_running.store(false, Ordering::SeqCst);
                return;
            }
        };

        // Let PipeWire negotiate the format before streaming; bail fast if the
        // user already left ScreenSync.
        if thread_running.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(300));
        }

        if thread_running.load(Ordering::SeqCst) {
            let _ = run_screen_color_loop(&keyboard, &thread_state, thread_running, false);
        }

        // Teardown on the shared runtime — the one that owns the ashpd/zbus
        // connection — which we intentionally leave running for the next capture.
        rt.block_on(capture.shutdown());
    });

    TuiScreenCapture {
        state,
        running,
        error,
        thread,
    }
}

// MonsGeek M1 V5 HE TUI Application
// Real-time monitoring and settings configuration

mod help;
mod keys;
mod shared;
mod tabs;
mod widgets;

use help::render_help_popup;
use shared::*;

use tabs::audio::AudioTabState;
use tabs::depth::{get_key_label, render_depth_monitor};
use tabs::device_info::{HexColorTarget, InfoTag, render_device_info};

use tabs::key_mapping::{KeyMappingFilter, KeyMappingView, KmSort};
use tabs::triggers::{TriggerEditModal, TriggerField, render_trigger_edit_modal};

#[cfg(feature = "notify")]
use crate::effect::{EffectLibrary, default_effects_path};
#[cfg(feature = "notify")]
use tabs::notify::{NotifyFocus, NotifyTabState, handle_notify_input, render_notify};

use crossterm::{
    ExecutableCommand,
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEventKind,
        MouseButton, MouseEventKind,
    },
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use ratatui::{prelude::*, widgets::*};
use std::cell::Cell as StdCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, stdout};
use std::sync::Arc;
use std::time::{Duration, Instant};
use throbber_widgets_tui::{BRAILLE_SIX, ThrobberState};
use tokio::sync::{broadcast, mpsc};
use tui_scrollview::ScrollViewState;

// Use shared library
use crate::firmware_api::FirmwareCheckResult;
use crate::hid::BatteryInfo;
use crate::keymap::{self, KeyRow};
use crate::power_supply::find_hid_battery_power_supply;
use crate::{FirmwareSettings, TriggerSettings, cmd, devices};

// Keyboard abstraction layer - using async interface directly
use monsgeek_keyboard::{
    Keyboard, Precision, RyKeyboard, SleepTimeSettings, TimestampedEvent, VendorEvent,
    led::speed_to_wire,
};
use monsgeek_transport::protocol::Profile;
use monsgeek_transport::{FlowControlTransport, HidDiscovery, Transport};

/// Resolve `(array_dim, display_count)` for a device.
///
/// Application state
struct App {
    /// Device selector from --device flag (index, transport name, or HID path)
    device_selector: Option<String>,
    info: FirmwareSettings,
    tab: usize,
    selected: usize,
    key_depths: Vec<f32>,
    depth_monitoring: bool,
    status_msg: String,
    connected: bool,
    /// Monotonically increasing generation counter; bumped on device switch.
    /// Async results carrying an older generation are silently discarded.
    device_generation: u64,
    device_name: String,
    transport_name: &'static str,
    /// Firmware scan-grid dimension (matrix_size): sizes per-key buffers and depth
    /// tracking. Includes empty scan cells — NOT the human key count.
    key_count: u8,
    /// Human-facing key count shown in the UI (vendor logical count, e.g. 83), distinct
    /// from `key_count`/`matrix_size` (the padded scan dimension, e.g. 90).
    display_key_count: u8,
    has_sidelight: bool,
    has_magnetism: bool,
    matrix_size: usize,
    matrix_key_names: Vec<String>,
    // Trigger settings
    triggers: Option<TriggerSettings>,
    trigger_scroll: usize,
    trigger_selected_key: usize, // Selected key in layout view
    precision: Precision,
    // Keyboard options
    options: Option<KeyboardOptions>,
    // Sleep time settings (loaded separately, merged into options)
    sleep_settings: Option<SleepTimeSettings>,
    // Remap tab state (tab 5)
    // Key Mapping tab state (unified per-key view)
    key_rows: Vec<KeyRow>,
    /// Which table an in-flight key-mapping load is on, for the progress bar
    key_rows_progress: Option<crate::keymap::LoadStage>,
    /// Page counter when the current key-mapping load started
    key_rows_pages_start: u64,
    key_mapping_selected: usize,
    key_mapping_view: KeyMappingView,
    key_mapping_sort: KmSort,
    key_mapping_filter: KeyMappingFilter,
    key_mapping_filter_open: bool,
    key_mapping_filter_field: usize,
    // Macro data (loaded alongside remaps or on editor open)
    // Key depth visualization
    depth_view_mode: DepthViewMode,
    depth_history: Vec<VecDeque<(f64, f32)>>, // Per-key history (timestamp, depth_mm)
    active_keys: HashSet<usize>,              // Keys with recent activity
    selected_keys: HashSet<usize>,            // Keys selected for time series view
    depth_cursor: usize,                      // Cursor for key selection
    max_observed_depth: f32,                  // Max depth observed during session (for bar scaling)
    depth_last_update: Vec<Instant>,          // Last update time per key (for stale detection)
    // Patch info (custom firmware capabilities)
    patch_info: Option<PatchInfoData>,
    // Dongle patch info (custom dongle firmware capabilities)
    dongle_patch_info: Option<PatchInfoData>,
    // Dongle info (for 2.4GHz dongle)
    dongle_info: Option<monsgeek_transport::DongleInfo>,
    dongle_status: Option<monsgeek_transport::DongleStatus>,
    rf_info: Option<monsgeek_transport::RfInfo>,
    // Battery status (for 2.4GHz dongle)
    battery: Option<BatteryInfo>,
    battery_source: Option<BatterySource>,
    last_battery_check: Instant,
    is_wireless: bool,
    // Polling rate: what this model supports, and the rates it accepts (fastest first).
    // Empty/Unsupported means the TUI shows the field as unavailable instead of editable.
    polling_rate_support: crate::device_loader::PollingRateSupport,
    polling_rates: &'static [u16],
    // Help popup
    show_help: bool,
    // Device picker popup
    show_device_picker: bool,
    device_picker_items: Vec<(
        monsgeek_transport::ProbedDevice,
        monsgeek_transport::DeviceLabel,
    )>,
    device_picker_selected: usize,
    // Keyboard interface (async, wrapped in Arc for spawning tasks)
    keyboard: Option<Arc<RyKeyboard>>,
    // Event receiver for low-latency EP2 notifications (with timestamps)
    event_rx: Option<broadcast::Receiver<TimestampedEvent>>,
    loading: LoadingStates,
    throbber_state: ThrobberState,
    // Async result channel (sender for spawned tasks)
    result_tx: mpsc::UnboundedSender<GenerationalResult>,
    // Hex color input
    hex_editing: bool,
    hex_input: String,
    hex_target: HexColorTarget,
    // Firmware check result
    firmware_check: Option<FirmwareCheckResult>,
    // Mouse hit areas (updated during render via interior mutability)
    tab_bar_area: StdCell<Rect>,
    content_area: StdCell<Rect>,
    // Scroll view state for content area
    scroll_state: ScrollViewState,
    // Trigger edit modal
    trigger_edit_modal: Option<TriggerEditModal>,
    // Device Info tab: tags for each list item (updated during render)
    info_tags: Vec<InfoTag>,
    // Notify tab state (feature-gated)
    #[cfg(feature = "notify")]
    notify: NotifyTabState,
    // Audio-reactive state (shown in the Device Info tab)
    audio: AudioTabState,
    // Screen-reactive state (shown in the Device Info tab)
    screen: tabs::screen::ScreenTabState,
    // LED mode selected before entering a reactive mode (music/screen), so it
    // can be restored on exit. `None` when not in a reactive mode.
    reactive_prev_mode: Option<u8>,
    // Selected UserPicture layer (mode 13) to display
    userpic_layer: u8,
    // Animation engine status (periodic query + interpolation)
    anim_snapshot: Option<crate::anim::EngineSnapshot>,
    anim_snapshot_time: Instant, // when the last snapshot was received
    anim_poll_interval: Duration,
    last_anim_poll: Instant,
}

impl App {
    fn new(device_selector: Option<String>) -> (Self, mpsc::UnboundedReceiver<GenerationalResult>) {
        let (result_tx, result_rx) = mpsc::unbounded_channel();
        let persisted = crate::settings::Settings::load();
        let app = Self {
            device_selector,
            key_rows_progress: None,
            key_rows_pages_start: 0,
            info: FirmwareSettings::default(),
            tab: 0,
            selected: 0,
            key_depths: Vec::new(),
            depth_monitoring: false,
            status_msg: String::new(),
            connected: false,
            device_generation: 0,
            device_name: String::new(),
            transport_name: "",
            key_count: 0,
            display_key_count: 0,
            has_sidelight: false,
            has_magnetism: false,
            matrix_size: 0,
            matrix_key_names: Vec::new(),
            triggers: None,
            trigger_scroll: 0,
            trigger_selected_key: 0,
            precision: Precision::default(),
            options: None,
            sleep_settings: None,
            key_rows: Vec::new(),
            key_mapping_selected: 0,
            key_mapping_view: KeyMappingView::default(),
            key_mapping_sort: KmSort::default(),
            key_mapping_filter: KeyMappingFilter::default(),
            key_mapping_filter_open: false,
            key_mapping_filter_field: 0,
            // Key depth visualization
            depth_view_mode: DepthViewMode::default(),
            depth_history: Vec::new(),
            active_keys: HashSet::new(),
            selected_keys: HashSet::new(),
            depth_cursor: 0,
            max_observed_depth: 0.1, // Will grow as keys are pressed
            depth_last_update: Vec::new(),
            // Patch info
            patch_info: None,
            dongle_patch_info: None,
            // Dongle info
            dongle_info: None,
            dongle_status: None,
            rf_info: None,
            // Battery status
            battery: None,
            battery_source: None,
            last_battery_check: Instant::now(),
            is_wireless: false,
            polling_rate_support: crate::device_loader::PollingRateSupport::Unsupported,
            polling_rates: &[],
            // Help popup
            show_help: false,
            // Device picker popup
            show_device_picker: false,
            device_picker_items: Vec::new(),
            device_picker_selected: 0,
            // Keyboard interface (wrapped in Arc for spawning tasks)
            keyboard: None,
            // Event receiver (subscribed on connect)
            event_rx: None,
            loading: LoadingStates::default(),
            throbber_state: ThrobberState::default(),
            result_tx,
            // Hex color input
            hex_editing: false,
            hex_input: String::new(),
            hex_target: HexColorTarget::default(),
            // Firmware check
            firmware_check: None,
            // Mouse hit areas (updated during render)
            tab_bar_area: StdCell::new(Rect::default()),
            content_area: StdCell::new(Rect::default()),
            // Scroll view state
            scroll_state: ScrollViewState::new(),
            // Trigger edit modal
            trigger_edit_modal: None,
            // Device Info list tags
            info_tags: Vec::new(),
            #[cfg(feature = "notify")]
            notify: NotifyTabState::default(),
            audio: AudioTabState::with_rate(persisted.audio_rate_hz),
            screen: tabs::screen::ScreenTabState::from_settings(&persisted),
            reactive_prev_mode: None,
            userpic_layer: 0,
            // Animation engine
            anim_snapshot: None,
            anim_snapshot_time: Instant::now(),
            anim_poll_interval: Duration::from_secs(1),
            last_anim_poll: Instant::now() - Duration::from_secs(10), // trigger immediate first poll
        };
        (app, result_rx)
    }

    /// Create a generation-tagged sender for spawning async tasks.
    fn gen_sender(&self) -> GenSender {
        GenSender {
            tx: self.result_tx.clone(),
            generation: self.device_generation,
        }
    }

    fn connect(&mut self) -> Result<(), String> {
        // Bump generation so in-flight async results from the old device are discarded
        self.device_generation += 1;

        // Use async device discovery with smart probing
        let discovery = HidDiscovery::new();

        // If --device was specified, use the resolve logic; otherwise auto-select
        let transport = if let Some(ref selector) = self.device_selector {
            use monsgeek_transport::DeviceDiscovery;
            let resolve_name = |device_id: Option<u32>, vid: u16, pid: u16| -> Option<String> {
                devices::get_device_info_with_id(device_id.map(|id| id as i32), vid, pid)
                    .map(|info| info.display_name)
            };
            let labeled = discovery
                .list_labeled_devices(resolve_name)
                .map_err(|e| format!("Discovery failed: {e}"))?;
            if labeled.is_empty() {
                return Err("No supported device found".into());
            }
            // Try index
            if let Ok(idx) = selector.parse::<usize>() {
                let (p, _) = labeled
                    .into_iter()
                    .find(|(_, l)| l.index == idx)
                    .ok_or_else(|| format!("Device index {idx} out of range"))?;
                discovery
                    .open_device(&p.device)
                    .map_err(|e| format!("Failed to open device: {e}"))?
            } else {
                // Try transport name
                let matches: Vec<_> = labeled
                    .iter()
                    .filter(|(_, l)| l.transport_name == selector.as_str())
                    .collect();
                if matches.len() == 1 {
                    discovery
                        .open_device(&matches[0].0.device)
                        .map_err(|e| format!("Failed to open device: {e}"))?
                } else if matches.len() > 1 {
                    return Err(format!(
                        "Ambiguous --device '{selector}': {} matches",
                        matches.len()
                    ));
                } else {
                    // Try path match
                    let path_matches: Vec<_> = labeled
                        .iter()
                        .filter(|(_, l)| l.hid_path.contains(selector.as_str()))
                        .collect();
                    if path_matches.len() == 1 {
                        discovery
                            .open_device(&path_matches[0].0.device)
                            .map_err(|e| format!("Failed to open device: {e}"))?
                    } else {
                        return Err(format!("No device matches '{selector}'"));
                    }
                }
            }
        } else {
            // Default: auto-select best device (TUI picks first, doesn't error on multiple)
            discovery
                .open_preferred()
                .map_err(|e| format!("Failed to open device: {e}"))?
        };

        let transport_info = transport.device_info().clone();
        let vid = transport_info.vid;
        let pid = transport_info.pid;

        let flow_transport = Arc::new(FlowControlTransport::new(transport));

        // Query device_id for accurate DB lookup (shared PIDs are ambiguous without it)
        let device_id = flow_transport
            .query_command(
                cmd::GET_USB_VERSION,
                &[],
                monsgeek_transport::ChecksumType::Bit7,
            )
            .ok()
            .filter(|r| r.len() >= 5 && r[0] == cmd::GET_USB_VERSION)
            .map(|r| u32::from_le_bytes([r[1], r[2], r[3], r[4]]) as i32);

        let db_key_count = devices::key_count_with_id(device_id, vid, pid);
        let has_magnetism = devices::has_magnetism_with_id(device_id, vid, pid);
        let device_info = devices::get_device_info_with_id(device_id, vid, pid);
        let has_sidelight = device_info
            .as_ref()
            .map(|d| d.has_sidelight)
            .unwrap_or(false);
        let device_name = device_info
            .as_ref()
            .map(|d| d.display_name.clone())
            .or_else(|| transport_info.product_name.clone())
            .unwrap_or_else(|| format!("Device {vid:04x}:{pid:04x}"));

        let protocol = monsgeek_transport::protocol::ProtocolFamily::detect(
            device_info.as_ref().map(|d| d.name.as_str()),
            pid,
        );

        // Try matrix database for key names and matrix size
        let registry = crate::profile_registry();
        let matrix_db: Option<&crate::device_loader::JsonDeviceMatrix> =
            device_id.and_then(|id| registry.get_device_matrix(vid, pid, id));
        let key_count = crate::device_loader::scan_extent(db_key_count, matrix_db);
        let display_key_count = crate::device_loader::display_key_count(db_key_count, matrix_db);

        let mut kb = RyKeyboard::new(flow_transport, key_count, has_magnetism, protocol);
        let (polling_rate_support, polling_rates) =
            resolve_polling_rate(device_id, vid, pid, transport_info.transport_type);
        kb.set_polling_rates(polling_rates.to_vec());

        if let Some(names) = registry.resolve_matrix_key_names(device_id, vid, pid) {
            kb.set_matrix_key_names(names);
        }
        if let Some(m) = matrix_db {
            kb.set_matrix_defaults(m.matrix.clone());
        }

        // Per-profile data is read and written against whichever profile the board
        // is actually running.
        kb.set_active_profile(kb.get_profile().unwrap_or_default());

        // Set non-analog positions from matrix database (encoder/GPIO keys).
        if let Some(matrix) = matrix_db
            && let Some(positions) = &matrix.non_analog_positions
        {
            kb.set_non_analog_positions(positions.clone());
        }

        let matrix_size = kb.matrix_size();
        let matrix_key_names: Vec<String> = (0..matrix_size)
            .map(|i| kb.matrix_key_name(i).to_string())
            .collect();

        let keyboard = Arc::new(kb);
        let is_wireless = keyboard.is_wireless();

        // Subscribe to low-latency event notifications
        self.event_rx = keyboard.subscribe_events();

        self.device_name = device_name;
        self.transport_name = transport_type_name(transport_info.transport_type);
        self.key_count = key_count;
        self.display_key_count = display_key_count;
        self.has_sidelight = has_sidelight;
        self.has_magnetism = has_magnetism;
        self.matrix_size = matrix_size;
        self.matrix_key_names = matrix_key_names;
        self.is_wireless = is_wireless;
        self.polling_rate_support = polling_rate_support;
        self.polling_rates = polling_rates;
        self.keyboard = Some(keyboard);

        // Initialize key depths array based on actual key count
        self.key_depths = vec![0.0; self.key_count as usize];
        // Initialize depth history for time series
        self.depth_history =
            vec![VecDeque::with_capacity(DEPTH_HISTORY_LEN); self.key_count as usize];
        // Initialize last update times (set to past so they don't show as active)
        self.depth_last_update = vec![Instant::now(); self.key_count as usize];
        self.active_keys.clear();
        self.selected_keys.clear();

        // Detect battery source (kernel power_supply if eBPF loaded, else vendor)
        if self.is_wireless {
            self.battery_source = if let Some(path) = find_hid_battery_power_supply(vid, pid) {
                Some(BatterySource::Kernel(path))
            } else {
                Some(BatterySource::Vendor)
            };
        }

        self.connected = true;

        // Load dongle info (instant dongle-local queries)
        if self.is_wireless
            && let Some(ref keyboard) = self.keyboard
        {
            let transport = keyboard.transport();
            self.dongle_info = transport.query_dongle_info().ok().flatten();
            self.dongle_status = transport.query_dongle_status().ok().flatten();
            self.rf_info = transport.query_rf_info().ok().flatten();
        }

        // Load battery status immediately for wireless devices
        if self.is_wireless {
            self.refresh_battery();
            // Show warning if keyboard is idle/sleeping
            if self.battery.as_ref().map(|b| b.idle).unwrap_or(false) {
                self.status_msg =
                    "Keyboard sleeping - press a key to wake before querying".to_string();
            } else {
                self.status_msg = format!("Connected to {}", self.device_name);
            }
        } else {
            self.status_msg = format!("Connected to {}", self.device_name);
        }

        Ok(())
    }

    /// Scan for devices and populate the device picker list.
    fn scan_device_picker(&mut self) {
        let discovery = HidDiscovery::new();
        let resolve_name = |device_id: Option<u32>, vid: u16, pid: u16| -> Option<String> {
            devices::get_device_info_with_id(device_id.map(|id| id as i32), vid, pid)
                .map(|info| info.display_name)
        };
        match discovery.list_labeled_devices(resolve_name) {
            Ok(items) => {
                self.device_picker_items = items;
                self.device_picker_selected = 0;
            }
            Err(e) => {
                self.status_msg = format!("Device scan failed: {e}");
                self.device_picker_items.clear();
            }
        }
    }

    /// Connect to the device currently selected in the device picker.
    fn connect_to_picked_device(&mut self) {
        use monsgeek_transport::DeviceDiscovery;

        // Bump generation so in-flight async results from the old device are discarded
        self.device_generation += 1;

        if self.device_picker_items.is_empty() {
            return;
        }
        let idx = self
            .device_picker_selected
            .min(self.device_picker_items.len() - 1);
        let (probed, label) = &self.device_picker_items[idx];

        let discovery = HidDiscovery::new();
        match discovery.open_device(&probed.device) {
            Ok(transport) => {
                // Set device_selector to the index so connect() logic is bypassed
                self.device_selector = Some(label.index.to_string());
                self.show_device_picker = false;

                // Now do the full connection setup with this transport
                let transport_info = transport.device_info().clone();
                let vid = transport_info.vid;
                let pid = transport_info.pid;
                let flow_transport = Arc::new(FlowControlTransport::new(transport));

                let device_id = flow_transport
                    .query_command(
                        cmd::GET_USB_VERSION,
                        &[],
                        monsgeek_transport::ChecksumType::Bit7,
                    )
                    .ok()
                    .filter(|r| r.len() >= 5 && r[0] == cmd::GET_USB_VERSION)
                    .map(|r| u32::from_le_bytes([r[1], r[2], r[3], r[4]]) as i32);

                let db_key_count = devices::key_count_with_id(device_id, vid, pid);
                let has_magnetism = devices::has_magnetism_with_id(device_id, vid, pid);
                let device_info = devices::get_device_info_with_id(device_id, vid, pid);
                let has_sidelight = device_info
                    .as_ref()
                    .map(|d| d.has_sidelight)
                    .unwrap_or(false);
                let device_name = device_info
                    .as_ref()
                    .map(|d| d.display_name.clone())
                    .or_else(|| transport_info.product_name.clone())
                    .unwrap_or_else(|| format!("Device {vid:04x}:{pid:04x}"));

                let protocol = monsgeek_transport::protocol::ProtocolFamily::detect(
                    device_info.as_ref().map(|d| d.name.as_str()),
                    pid,
                );

                let registry = crate::profile_registry();
                let matrix_db = device_id.and_then(|id| registry.get_device_matrix(vid, pid, id));
                let key_count = crate::device_loader::scan_extent(db_key_count, matrix_db);
                let display_key_count =
                    crate::device_loader::display_key_count(db_key_count, matrix_db);

                let mut kb = RyKeyboard::new(flow_transport, key_count, has_magnetism, protocol);
                let (polling_rate_support, polling_rates) =
                    resolve_polling_rate(device_id, vid, pid, transport_info.transport_type);
                kb.set_polling_rates(polling_rates.to_vec());

                if let Some(names) = registry.resolve_matrix_key_names(device_id, vid, pid) {
                    kb.set_matrix_key_names(names);
                }
                if let Some(m) = matrix_db {
                    kb.set_matrix_defaults(m.matrix.clone());
                }

                kb.set_active_profile(kb.get_profile().unwrap_or_default());

                if let Some(matrix) = matrix_db
                    && let Some(positions) = &matrix.non_analog_positions
                {
                    kb.set_non_analog_positions(positions.clone());
                }

                let matrix_size = kb.matrix_size();
                let matrix_key_names: Vec<String> = (0..matrix_size)
                    .map(|i| kb.matrix_key_name(i).to_string())
                    .collect();

                let keyboard = Arc::new(kb);
                let is_wireless = keyboard.is_wireless();

                self.event_rx = keyboard.subscribe_events();
                self.device_name = device_name;
                self.transport_name = transport_type_name(transport_info.transport_type);
                self.key_count = key_count;
                self.display_key_count = display_key_count;
                self.has_sidelight = has_sidelight;
                self.has_magnetism = has_magnetism;
                self.matrix_size = matrix_size;
                self.matrix_key_names = matrix_key_names;
                self.is_wireless = is_wireless;
                self.polling_rate_support = polling_rate_support;
                self.polling_rates = polling_rates;
                self.keyboard = Some(keyboard);

                self.key_depths = vec![0.0; self.key_count as usize];
                self.depth_history =
                    vec![VecDeque::with_capacity(DEPTH_HISTORY_LEN); self.key_count as usize];
                self.depth_last_update = vec![Instant::now(); self.key_count as usize];
                self.active_keys.clear();
                self.selected_keys.clear();

                if self.is_wireless {
                    self.battery_source =
                        if let Some(path) = find_hid_battery_power_supply(vid, pid) {
                            Some(BatterySource::Kernel(path))
                        } else {
                            Some(BatterySource::Vendor)
                        };
                }

                self.connected = true;

                if self.is_wireless {
                    if let Some(ref keyboard) = self.keyboard {
                        let transport = keyboard.transport();
                        self.dongle_info = transport.query_dongle_info().ok().flatten();
                        self.dongle_status = transport.query_dongle_status().ok().flatten();
                        self.rf_info = transport.query_rf_info().ok().flatten();
                    }
                    self.refresh_battery();
                }

                // Reset loading states so tabs re-fetch
                self.loading = LoadingStates::default();
                self.info = FirmwareSettings::default();
                self.triggers = None;
                self.patch_info = None;
                self.dongle_patch_info = None;
                self.firmware_check = None;
                self.anim_snapshot = None;

                self.status_msg = format!("Connected to {}", self.device_name);
                self.load_device_info();
            }
            Err(e) => {
                self.status_msg = format!("Failed to open device: {e}");
                self.show_device_picker = false;
            }
        }
    }
    /// Load the unified per-key rows for the Key Mapping tab.
    fn load_key_mapping(&mut self) {
        let Some(keyboard) = self.keyboard.clone() else {
            return;
        };
        self.loading.key_mapping = LoadState::Loading;
        // Baseline for the per-page counter the progress bar interpolates with.
        self.key_rows_pages_start = keyboard.pages_read();
        self.key_rows_progress = None;
        let tx = self.gen_sender();
        tokio::spawn(async move {
            let stage_tx = tx.clone();
            let on_stage = move |s| stage_tx.send(AsyncResult::KeyRowsStage(s));
            match keymap::load_key_rows(&keyboard, 0, &on_stage) {
                Ok(rows) => tx.send(AsyncResult::KeyRows(Ok(rows))),
                Err(e) => tx.send(AsyncResult::KeyRows(Err(e.to_string()))),
            }
        });
    }

    /// Process async result from background tasks
    fn process_async_result(&mut self, result: AsyncResult) {
        match result {
            AsyncResult::DeviceIdAndVersion(Ok((device_id, ver))) => {
                self.info.device_id = device_id;
                self.info.version = ver.raw;
                self.loading.usb_version = LoadState::Loaded;
            }
            AsyncResult::DeviceIdAndVersion(Err(_)) => {
                self.loading.usb_version = LoadState::Error;
            }
            AsyncResult::Profile(Ok(p)) => {
                self.info.profile = p;
                self.loading.profile = LoadState::Loaded;
            }
            AsyncResult::Profile(Err(_)) => {
                self.loading.profile = LoadState::Error;
            }
            AsyncResult::Debounce(Ok(d)) => {
                self.info.debounce = d;
                self.loading.debounce = LoadState::Loaded;
            }
            AsyncResult::Debounce(Err(_)) => {
                self.loading.debounce = LoadState::Error;
            }
            AsyncResult::PollingRate(Ok(rate)) => {
                self.info.polling_rate = rate;
                self.loading.polling_rate = LoadState::Loaded;
            }
            AsyncResult::PollingRate(Err(_)) => {
                self.loading.polling_rate = LoadState::Error;
            }
            AsyncResult::LedParams(Ok(params)) => {
                self.info.led_mode = params.mode as u8;
                self.info.led_brightness = params.brightness;
                self.info.led_speed = params.speed;
                self.info.led_dazzle = params.direction == 7; // DAZZLE_ON=7
                self.info.led_r = params.color.r;
                self.info.led_g = params.color.g;
                self.info.led_b = params.color.b;
                self.loading.led_params = LoadState::Loaded;
            }
            AsyncResult::LedParams(Err(_)) => {
                self.loading.led_params = LoadState::Error;
            }
            AsyncResult::SideLedParams(Ok(params)) => {
                self.info.side_mode = params.mode as u8;
                self.info.side_brightness = params.brightness;
                self.info.side_speed = params.speed;
                self.info.side_dazzle = params.direction == 7;
                self.info.side_r = params.color.r;
                self.info.side_g = params.color.g;
                self.info.side_b = params.color.b;
                self.loading.side_led_params = LoadState::Loaded;
            }
            AsyncResult::SideLedParams(Err(_)) => {
                self.loading.side_led_params = LoadState::Error;
            }
            AsyncResult::KbOptions(Ok(opts)) => {
                self.info.fn_layer = opts.fn_layer;
                self.info.wasd_swap = opts.wasd_swap;
                self.loading.kb_options_info = LoadState::Loaded;
            }
            AsyncResult::KbOptions(Err(_)) => {
                self.loading.kb_options_info = LoadState::Error;
            }
            AsyncResult::Precision(Ok(precision)) => {
                self.precision = precision;
                self.loading.precision = LoadState::Loaded;
            }
            AsyncResult::Precision(Err(_)) => {
                self.loading.precision = LoadState::Error;
            }
            AsyncResult::SleepTime(Ok(settings)) => {
                // Store full sleep time settings
                self.info.sleep_seconds = settings.idle_bt; // For info display
                self.sleep_settings = Some(settings);
                self.loading.sleep_time = LoadState::Loaded;
                // Update options if already loaded
                if let Some(ref mut opts) = self.options
                    && let Some(ref s) = self.sleep_settings
                {
                    opts.idle_bt = s.idle_bt;
                    opts.idle_24g = s.idle_24g;
                    opts.deep_bt = s.deep_bt;
                    opts.deep_24g = s.deep_24g;
                }
            }
            AsyncResult::SleepTime(Err(_)) => {
                self.loading.sleep_time = LoadState::Error;
            }
            AsyncResult::PatchInfo(Ok(data)) => {
                self.patch_info = Some(data);
                self.loading.patch_info = LoadState::Loaded;
            }
            AsyncResult::PatchInfo(Err(_)) => {
                self.loading.patch_info = LoadState::Error;
            }
            AsyncResult::DonglePatchInfo(Ok(data)) => {
                self.dongle_patch_info = Some(data);
                self.loading.dongle_patch_info = LoadState::Loaded;
            }
            AsyncResult::DonglePatchInfo(Err(_)) => {
                self.loading.dongle_patch_info = LoadState::Loaded; // Not an error, just not available
            }
            AsyncResult::FirmwareCheck(result) => {
                self.firmware_check = Some(result.clone());
                self.loading.firmware_check = LoadState::Loaded;
                self.status_msg = result.message;
            }
            AsyncResult::Triggers(Ok(triggers)) => {
                self.triggers = Some(triggers);
                self.loading.triggers = LoadState::Loaded;
                self.status_msg = "Trigger settings loaded".to_string();
            }
            AsyncResult::Triggers(Err(_)) => {
                self.loading.triggers = LoadState::Error;
                self.status_msg = "Failed to load trigger settings".to_string();
            }
            AsyncResult::Options(Ok(opts)) => {
                // Get sleep settings if already loaded, otherwise use defaults
                let sleep = self.sleep_settings.unwrap_or_default();
                self.options = Some(KeyboardOptions {
                    os_mode: opts.os_mode,
                    fn_layer: opts.fn_layer,
                    anti_mistouch: opts.anti_mistouch,
                    rt_stability: opts.rt_stability,
                    wasd_swap: opts.wasd_swap,
                    idle_bt: sleep.idle_bt,
                    idle_24g: sleep.idle_24g,
                    deep_bt: sleep.deep_bt,
                    deep_24g: sleep.deep_24g,
                });
                self.loading.options = LoadState::Loaded;
                self.status_msg = "Keyboard options loaded".to_string();
            }
            AsyncResult::Options(Err(_)) => {
                self.loading.options = LoadState::Error;
                self.status_msg = "Failed to load options".to_string();
            }
            AsyncResult::KeyRowsStage(stage) => {
                self.key_rows_progress = Some(stage);
            }
            AsyncResult::KeyRows(Ok(rows)) => {
                self.key_rows_progress = None;
                self.key_rows = rows;
                self.loading.key_mapping = LoadState::Loaded;
                if self.key_mapping_selected >= self.key_rows.len() {
                    self.key_mapping_selected = self.key_rows.len().saturating_sub(1);
                }
                self.status_msg = format!("{} keys loaded", self.key_rows.len());
            }
            AsyncResult::KeyRows(Err(e)) => {
                self.key_rows_progress = None;
                self.loading.key_mapping = LoadState::Error;
                self.status_msg = format!("Failed to load key mapping: {e}");
            }
            AsyncResult::SetComplete(field, Ok(())) => {
                self.status_msg = format!("{field} updated");
            }
            AsyncResult::SetComplete(field, Err(e)) => {
                self.status_msg = format!("Failed to set {field}: {e}");
            }
            // Battery is read synchronously via feature report, not used currently
            AsyncResult::Battery(Ok(info)) => {
                self.battery = Some(info);
            }
            AsyncResult::Battery(Err(e)) => {
                self.status_msg = format!("Battery read failed: {e}");
            }
            // Notify tab results
            #[cfg(feature = "notify")]
            AsyncResult::NotifyEffectsLoaded(Ok(lib)) => {
                self.notify.effect_names = lib.names().into_iter().map(String::from).collect();
                self.notify.effects = Some(lib);
                self.notify.selected_effect = 0;
                self.notify_recompute_preview();
                self.status_msg = format!(
                    "Loaded {} effects from {}",
                    self.notify.effect_names.len(),
                    default_effects_path().display()
                );
            }
            #[cfg(feature = "notify")]
            AsyncResult::NotifyEffectsLoaded(Err(e)) => {
                self.status_msg = format!("Failed to load effects: {e}");
            }
            #[cfg(feature = "notify")]
            AsyncResult::NotifyDaemonStopped(Ok(())) => {
                self.notify.daemon_running = false;
                self.notify.daemon_handle = None;
                self.notify.daemon_cancel = None;
                self.status_msg = "Notify daemon stopped".to_string();
            }
            #[cfg(feature = "notify")]
            AsyncResult::NotifyDaemonStopped(Err(e)) => {
                self.notify.daemon_running = false;
                self.notify.daemon_handle = None;
                self.notify.daemon_cancel = None;
                self.notify.daemon_error = Some(e.clone());
                self.status_msg = format!("Daemon error: {e}");
            }
            #[cfg(feature = "notify")]
            AsyncResult::NotifyList(list) => {
                self.notify.notifications = list;
            }
            AsyncResult::AnimStatus(Ok(snap)) => {
                self.anim_snapshot = Some(snap);
                self.anim_snapshot_time = Instant::now();
            }
            AsyncResult::AnimStatus(Err(_)) => {
                // Firmware doesn't support it — stop polling
                self.anim_snapshot = None;
            }
        }
    }

    /// Get current spinner character for inline display
    fn spinner_char(&self) -> &'static str {
        let idx = self.throbber_state.index() as usize % BRAILLE_SIX.symbols.len();
        BRAILLE_SIX.symbols[idx]
    }

    /// Number of tabs (depends on feature flags).
    fn tab_count(&self) -> usize {
        #[cfg(feature = "notify")]
        {
            5
        }
        #[cfg(not(feature = "notify"))]
        {
            4
        }
    }

    /// Tab names for display.
    fn tab_names(&self) -> Vec<&'static str> {
        let mut names = vec!["Device Info", "Key Depth", "Key Mapping"];
        #[cfg(feature = "notify")]
        names.push("Notify");
        names
    }

    /// Auto-load data when entering a tab.
    fn auto_load_tab(&mut self) {
        if self.tab == 2 && self.loading.key_mapping == LoadState::NotLoaded {
            self.load_key_mapping();
            // The unified editor reuses the trigger modal, which reads `triggers`.
            if self.loading.triggers == LoadState::NotLoaded {
                self.load_triggers();
            }
        }
        #[cfg(feature = "notify")]
        if self.tab == 3 && self.notify.effects.is_none() {
            self.load_notify_effects();
        }
    }

    // ── Notify tab methods ──────────────────────────────────────────────

    #[cfg(feature = "notify")]
    fn load_notify_effects(&mut self) {
        let tx = self.gen_sender();
        tokio::spawn(async move {
            let result = EffectLibrary::load_default();
            tx.send(AsyncResult::NotifyEffectsLoaded(result));
        });
    }
}

/// Run the TUI - called via 'iot_driver tui' command
pub async fn run(device_selector: Option<String>) -> io::Result<()> {
    use crossterm::event::KeyModifiers;

    // Initialize tui-logger for notify daemon log widget
    tui_logger::init_logger(log::LevelFilter::Info).ok();
    tui_logger::set_default_level(log::LevelFilter::Off);
    tui_logger::set_level_for_target("notify", log::LevelFilter::Info);

    // Setup terminal
    enable_raw_mode()?;
    stdout()
        .execute(EnterAlternateScreen)?
        .execute(EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let (mut app, mut result_rx) = App::new(device_selector);

    // Try to connect
    if let Err(e) = app.connect() {
        app.status_msg = e;
    } else {
        // Skip loading if keyboard is sleeping (queries will fail/timeout)
        let is_idle = app.battery.as_ref().map(|b| b.idle).unwrap_or(false);
        if !is_idle {
            // Load device info (TUI starts on tab 0) - spawns background tasks
            app.load_device_info();
            // Also load full options (needed for editable options on tab 0)
            app.load_options();
        }
    }

    // Set up async event stream
    let mut event_stream = EventStream::new();
    let mut tick_interval = tokio::time::interval(Duration::from_millis(100));
    let mut last_tick_ms = 100u64;

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        tokio::select! {
            // Handle async results from background tasks
            Some(gr) = result_rx.recv() => {
                // Discard results from a previous device generation
                if gr.generation == app.device_generation {
                    app.process_async_result(gr.result);
                }
            }
            // Handle terminal events
            maybe_event = event_stream.next() => {
                if let Some(Ok(Event::Key(key))) = maybe_event {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    // Device picker popup handling
                    if app.show_device_picker {
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('d') => {
                                app.show_device_picker = false;
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                if app.device_picker_selected > 0 {
                                    app.device_picker_selected -= 1;
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                if app.device_picker_selected + 1
                                    < app.device_picker_items.len()
                                {
                                    app.device_picker_selected += 1;
                                }
                            }
                            KeyCode::Enter => {
                                app.connect_to_picked_device();
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // Help popup handling
                    if app.show_help {
                        match key.code {
                            KeyCode::Char('?') | KeyCode::Esc | KeyCode::F(1) => {
                                app.show_help = false;
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // Hex color input mode
                    if app.hex_editing {
                        match key.code {
                            KeyCode::Esc => app.cancel_hex_input(),
                            KeyCode::Enter => app.apply_hex_input(),
                            KeyCode::Backspace => {
                                app.hex_input.pop();
                            }
                            KeyCode::Char(c)
                                if c.is_ascii_hexdigit() && app.hex_input.len() < 6 =>
                            {
                                app.hex_input.push(c.to_ascii_uppercase());
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // A picker popup (base-mode, Snap-Tap partner, or DKS) takes
                    // precedence over the modal's own field navigation.
                    let picker_open = app
                        .trigger_edit_modal
                        .as_ref()
                        .is_some_and(|m| {
                            m.mode_picker.is_some()
                                || m.key_picker.is_some()
                                || m.output_picker.is_some()
                                || m.dks_action_picker.is_some()
                        });
                    if picker_open {
                        let enter = key.code == KeyCode::Enter;
                        if enter {
                            enum PickerConfirm {
                                Mode,
                                Key,
                                Output,
                                DksAction,
                            }
                            let confirm = app.trigger_edit_modal.as_ref().and_then(|m| {
                                if m.mode_picker.is_some() {
                                    Some(PickerConfirm::Mode)
                                } else if m.key_picker.is_some() {
                                    Some(PickerConfirm::Key)
                                } else if m.output_picker.is_some() {
                                    Some(PickerConfirm::Output)
                                } else if m.dks_action_picker.is_some() {
                                    Some(PickerConfirm::DksAction)
                                } else {
                                    None
                                }
                            });
                            match confirm {
                                Some(PickerConfirm::Mode) => {
                                    if let Some(m) = app.trigger_edit_modal.as_mut() {
                                        m.confirm_mode_picker();
                                    }
                                }
                                Some(PickerConfirm::Key) => {
                                    if let Some(m) = app.trigger_edit_modal.as_mut() {
                                        m.confirm_key_picker();
                                    }
                                }
                                Some(PickerConfirm::Output) => {
                                    // The picker carries the chosen KeyAction directly
                                    // (a HID key, a consumer/media control, or Disabled
                                    // for "(none)"). Which entry it lands in depends on
                                    // the field that opened it — both are keymatrix
                                    // entries, so one picker serves both.
                                    let picked = app
                                        .trigger_edit_modal
                                        .as_mut()
                                        .and_then(|m| m.output_picker.take())
                                        .and_then(|p| p.selected().copied())
                                        .map(|highlighted| {
                                            let m = app.trigger_edit_modal.as_ref().unwrap();
                                            m.picked_action(highlighted)
                                        });
                                    if let (Some(action), Some(m)) =
                                        (picked, app.trigger_edit_modal.as_mut())
                                    {
                                        if m.current_field() == TriggerField::DksBindingKey {
                                            let slot = m.dks_binding_index;
                                            m.slots[slot] = action;
                                        } else {
                                            m.set_output_action(action);
                                        }
                                    }
                                }
                                Some(PickerConfirm::DksAction) => {
                                    if let Some(m) = app.trigger_edit_modal.as_mut() {
                                        m.confirm_dks_action_picker();
                                    }
                                }
                                None => {}
                            }
                            continue;
                        }
                        if let Some(ref mut modal) = app.trigger_edit_modal {
                            let up = matches!(key.code, KeyCode::Up | KeyCode::Char('k'));
                            let down = matches!(key.code, KeyCode::Down | KeyCode::Char('j'));
                            if let Some(p) = modal.mode_picker.as_mut() {
                                match key.code {
                                    _ if up => p.up(),
                                    _ if down => p.down(),
                                    KeyCode::Esc => modal.mode_picker = None,
                                    _ => {}
                                }
                            } else if let Some(p) = modal.key_picker.as_mut() {
                                match key.code {
                                    _ if up => p.up(),
                                    _ if down => p.down(),
                                    KeyCode::Esc => modal.key_picker = None,
                                    _ => {}
                                }
                            } else if modal.output_picker.is_some() {
                                // Long list — type to filter, so only arrows navigate
                                // (j/k/letters are filter input) and Tab stages a
                                // chord key rather than moving focus.
                                if key.code == KeyCode::Tab {
                                    modal.toggle_chord_key();
                                } else if let Some(p) = modal.output_picker.as_mut() {
                                    match key.code {
                                        KeyCode::Up => p.up(),
                                        KeyCode::Down => p.down(),
                                        KeyCode::Backspace => p.pop_filter(),
                                        KeyCode::Char(c) => p.push_filter(c),
                                        KeyCode::Esc => modal.output_picker = None,
                                        _ => {}
                                    }
                                }
                            } else if let Some((_, p)) = modal.dks_action_picker.as_mut() {
                                match key.code {
                                    _ if up => p.up(),
                                    _ if down => p.down(),
                                    KeyCode::Esc => modal.dks_action_picker = None,
                                    _ => {}
                                }
                            }
                        }
                        continue;
                    }

                    // Key Mapping filter popup: ↑↓ pick field, ←→ change, Esc/Enter/f close.
                    if app.key_mapping_filter_open {
                        match key.code {
                            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('f') => {
                                app.key_mapping_filter_open = false;
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.key_mapping_filter_field =
                                    app.key_mapping_filter_field.saturating_sub(1);
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                app.key_mapping_filter_field =
                                    (app.key_mapping_filter_field + 1).min(3);
                            }
                            KeyCode::Left | KeyCode::Char('h') => {
                                tabs::key_mapping::cycle_filter_field(&mut app, false);
                            }
                            KeyCode::Right | KeyCode::Char('l') => {
                                tabs::key_mapping::cycle_filter_field(&mut app, true);
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // Trigger edit modal - uses spinners with Left/Right to adjust values
                    if app.trigger_edit_modal.is_some() {
                        let coarse = key.modifiers.contains(KeyModifiers::SHIFT);
                        match key.code {
                            KeyCode::Esc => app.close_trigger_edit_modal(),
                            // Enter activates the focused row — opens its picker,
                            // flips a toggle, or commits from the Save button.
                            KeyCode::Enter => {
                                let save = app
                                    .trigger_edit_modal
                                    .as_mut()
                                    .is_some_and(|m| m.activate_current());
                                if save {
                                    app.save_trigger_edit_modal();
                                }
                            }
                            // Save from anywhere in the modal.
                            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.save_trigger_edit_modal();
                            }
                            KeyCode::Tab | KeyCode::Down => {
                                if let Some(ref mut modal) = app.trigger_edit_modal {
                                    modal.next_field();
                                }
                            }
                            KeyCode::BackTab | KeyCode::Up => {
                                if let Some(ref mut modal) = app.trigger_edit_modal {
                                    modal.prev_field();
                                }
                            }
                            KeyCode::Left | KeyCode::Char('h') => {
                                if let Some(ref mut modal) = app.trigger_edit_modal {
                                    modal.decrement_current(coarse);
                                }
                            }
                            KeyCode::Right | KeyCode::Char('l') => {
                                if let Some(ref mut modal) = app.trigger_edit_modal {
                                    modal.increment_current(coarse);
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // Alt-1..5 tab shortcuts
                    if key.modifiers.contains(crossterm::event::KeyModifiers::ALT) {
                        let tab_idx = match key.code {
                            KeyCode::Char('1') => Some(0),
                            KeyCode::Char('2') => Some(1),
                            KeyCode::Char('3') => Some(2),
                            KeyCode::Char('4') => Some(3),
                            KeyCode::Char('5') => Some(4),
                            _ => None,
                        };
                        if let Some(idx) = tab_idx {
                            if idx < app.tab_count() {
                                app.tab = idx;
                                app.selected = 0;
                                app.trigger_scroll = 0;
                                app.scroll_state = ScrollViewState::new();
                                app.auto_load_tab();
                            }
                            continue;
                        }
                    }

                    match key.code {
                        // Help toggle
                        KeyCode::Char('?') | KeyCode::F(1) => {
                            app.show_help = true;
                        }
                        KeyCode::Char('q') => break,
                        KeyCode::Esc => {
                            #[cfg(feature = "notify")]
                            if app.tab == 3 && app.notify.focus != NotifyFocus::EffectList {
                                handle_notify_input(&mut app, key.code);
                            } else {
                                break;
                            }
                            #[cfg(not(feature = "notify"))]
                            break;
                        }
                        // Tab/BackTab: navigate within current tab
                        KeyCode::Tab | KeyCode::BackTab => {
                            #[cfg(feature = "notify")]
                            if app.tab == 3 {
                                handle_notify_input(&mut app, key.code);
                            }
                            // Other tabs: no-op for now (can add widget focus later)
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.tab == 1 && app.depth_view_mode == DepthViewMode::BarChart {
                                let row_starts = [0, 15, 30, 43, 56];
                                if let Some(row) = row_starts.iter().rposition(|&s| s <= app.depth_cursor)
                                    && row > 0 {
                                        let col = app.depth_cursor - row_starts[row];
                                        let prev_row_start = row_starts[row - 1];
                                        let prev_row_size = row_starts[row] - prev_row_start;
                                        app.depth_cursor = prev_row_start + col.min(prev_row_size - 1);
                                    }
                            } else if app.tab == 2 {
                                if app.key_mapping_view == KeyMappingView::Layout {
                                    tabs::key_mapping::layout_move(&mut app, 0, -1);
                                } else if app.key_mapping_selected > 0 {
                                    app.key_mapping_selected -= 1;
                                    app.scroll_state.scroll_up();
                                }
                            } else {
                                #[cfg(feature = "notify")]
                                if app.tab == 3 {
                                    handle_notify_input(&mut app, key.code);
                                } else if app.selected > 0 {
                                    app.selected -= 1;
                                }
                                #[cfg(not(feature = "notify"))]
                                if app.selected > 0 {
                                    app.selected -= 1;
                                }
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.tab == 1 && app.depth_view_mode == DepthViewMode::BarChart {
                                let row_starts = [0, 15, 30, 43, 56, 66];
                                if let Some(row) = row_starts.iter().rposition(|&s| s <= app.depth_cursor)
                                    && row < 4 {
                                        let col = app.depth_cursor - row_starts[row];
                                        let next_row_start = row_starts[row + 1];
                                        let next_row_size = row_starts[row + 2] - next_row_start;
                                        app.depth_cursor = next_row_start + col.min(next_row_size - 1);
                                    }
                            } else if app.tab == 2 {
                                if app.key_mapping_view == KeyMappingView::Layout {
                                    tabs::key_mapping::layout_move(&mut app, 0, 1);
                                } else if app.key_mapping_selected + 1
                                    < tabs::key_mapping::visible_indices(&app).len()
                                {
                                    app.key_mapping_selected += 1;
                                    app.scroll_state.scroll_down();
                                }
                            } else {
                                #[cfg(feature = "notify")]
                                if app.tab == 3 {
                                    handle_notify_input(&mut app, key.code);
                                } else if app.selected < app.info_tags.len().saturating_sub(1) {
                                    app.selected += 1;
                                }
                                #[cfg(not(feature = "notify"))]
                                if app.selected < app.info_tags.len().saturating_sub(1) {
                                    app.selected += 1;
                                }
                            }
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            if app.tab == 1 && app.depth_view_mode == DepthViewMode::BarChart {
                                if app.depth_cursor > 0 {
                                    app.depth_cursor -= 1;
                                }
                            } else if app.tab == 2 && app.key_mapping_view == KeyMappingView::Layout {
                                tabs::key_mapping::layout_move(&mut app, -1, 0);
                            } else if app.tab == 0 {
                                let coarse = key.modifiers.contains(KeyModifiers::SHIFT);
                                match app.info_tags.get(app.selected).copied().unwrap_or(InfoTag::ReadOnly) {
                                    InfoTag::Profile => app.step_profile(-1),
                                    InfoTag::Debounce => app.set_debounce(DEBOUNCE_SPINNER.decrement_u8(app.info.debounce, coarse)),
                                    InfoTag::PollingRate => app.cycle_polling_rate(1), // higher index = lower rate
                                    InfoTag::LedMode => app.set_led_mode(app.info.led_mode.saturating_sub(1)),
                                    InfoTag::LedBrightness => app.set_brightness(BRIGHTNESS_SPINNER.decrement_u8(app.info.led_brightness, coarse)),
                                    InfoTag::LedSpeed => {
                                        let current = speed_to_wire(app.info.led_speed);
                                        app.set_speed(SPEED_SPINNER.decrement_u8(current, coarse));
                                    }
                                    InfoTag::LedRed => { let r = RGB_SPINNER.decrement_u8(app.info.led_r, coarse); app.set_color(r, app.info.led_g, app.info.led_b); }
                                    InfoTag::LedGreen => { let g = RGB_SPINNER.decrement_u8(app.info.led_g, coarse); app.set_color(app.info.led_r, g, app.info.led_b); }
                                    InfoTag::LedBlue => { let b = RGB_SPINNER.decrement_u8(app.info.led_b, coarse); app.set_color(app.info.led_r, app.info.led_g, b); }
                                    InfoTag::LedDazzle => { app.toggle_dazzle(); tabs::audio::reapply_if_active(&mut app); }
                                    InfoTag::SideMode => app.set_side_mode(app.info.side_mode.saturating_sub(1)),
                                    InfoTag::SideBrightness => app.set_side_brightness(BRIGHTNESS_SPINNER.decrement_u8(app.info.side_brightness, coarse)),
                                    InfoTag::SideSpeed => {
                                        let current = speed_to_wire(app.info.side_speed);
                                        app.set_side_speed(SPEED_SPINNER.decrement_u8(current, coarse));
                                    }
                                    InfoTag::SideRed => { let r = RGB_SPINNER.decrement_u8(app.info.side_r, coarse); app.set_side_color(r, app.info.side_g, app.info.side_b); }
                                    InfoTag::SideGreen => { let g = RGB_SPINNER.decrement_u8(app.info.side_g, coarse); app.set_side_color(app.info.side_r, g, app.info.side_b); }
                                    InfoTag::SideBlue => { let b = RGB_SPINNER.decrement_u8(app.info.side_b, coarse); app.set_side_color(app.info.side_r, app.info.side_g, b); }
                                    InfoTag::SideDazzle => app.toggle_side_dazzle(),
                                    InfoTag::OsMode => { if let Some(ref opts) = app.options.clone() { app.set_os_mode(OS_MODE_SPINNER.decrement_u8(opts.os_mode, coarse)); } }
                                    InfoTag::FnLayer => { if let Some(ref opts) = app.options.clone() { app.set_fn_layer(FN_LAYER_SPINNER.decrement_u8(opts.fn_layer, coarse)); } }
                                    InfoTag::WasdSwap => app.toggle_wasd_swap(),
                                    InfoTag::AntiMistouch => app.toggle_anti_mistouch(),
                                    InfoTag::RtStability => { if let Some(ref opts) = app.options.clone() { app.set_rt_stability(RT_STABILITY_SPINNER.decrement_u8(opts.rt_stability, coarse)); } }
                                    InfoTag::SleepIdleBt => { let step = if coarse { SLEEP_TIME_SPINNER.step_coarse } else { SLEEP_TIME_SPINNER.step } as i32; app.update_sleep_time(SleepField::IdleBt, -step); }
                                    InfoTag::SleepIdle24g => { let step = if coarse { SLEEP_TIME_SPINNER.step_coarse } else { SLEEP_TIME_SPINNER.step } as i32; app.update_sleep_time(SleepField::Idle24g, -step); }
                                    InfoTag::SleepDeepBt => { let step = if coarse { SLEEP_TIME_SPINNER.step_coarse } else { SLEEP_TIME_SPINNER.step } as i32; app.update_sleep_time(SleepField::DeepBt, -step); }
                                    InfoTag::SleepDeep24g => { let step = if coarse { SLEEP_TIME_SPINNER.step_coarse } else { SLEEP_TIME_SPINNER.step } as i32; app.update_sleep_time(SleepField::Deep24g, -step); }
                                    InfoTag::AudioDevice => tabs::audio::cycle_device(&mut app, -1),
                                    InfoTag::AudioVizStyle => tabs::audio::cycle_style(&mut app, -1),
                                    InfoTag::AudioRate => tabs::audio::cycle_rate(&mut app, -1),
                                    InfoTag::ScreenRate => tabs::screen::cycle_rate(&mut app, -1),
                                    InfoTag::ScreenRegion => tabs::screen::cycle_region(&mut app, -1),
                                    InfoTag::ScreenTestSwatch => tabs::screen::cycle_test_swatch(&mut app, -1),
                                    InfoTag::ScreenGainR => tabs::screen::adjust_calibration(&mut app, tabs::screen::CalField::GainR, -1, coarse),
                                    InfoTag::ScreenGainG => tabs::screen::adjust_calibration(&mut app, tabs::screen::CalField::GainG, -1, coarse),
                                    InfoTag::ScreenGainB => tabs::screen::adjust_calibration(&mut app, tabs::screen::CalField::GainB, -1, coarse),
                                    InfoTag::ScreenGammaR => tabs::screen::adjust_calibration(&mut app, tabs::screen::CalField::GammaR, -1, coarse),
                                    InfoTag::ScreenGammaG => tabs::screen::adjust_calibration(&mut app, tabs::screen::CalField::GammaG, -1, coarse),
                                    InfoTag::ScreenGammaB => tabs::screen::adjust_calibration(&mut app, tabs::screen::CalField::GammaB, -1, coarse),
                                    InfoTag::ScreenSaturation => tabs::screen::adjust_calibration(&mut app, tabs::screen::CalField::Saturation, -1, coarse),
                                    InfoTag::UserPicLayer => {
                                        let n = (app.userpic_layer + 3) % 4;
                                        app.set_userpic_layer(n);
                                    }
                                    _ => {}
                                }
                            }
                            #[cfg(feature = "notify")]
                            if app.tab == 3 {
                                handle_notify_input(&mut app, key.code);
                            }
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            if app.tab == 2 && app.key_mapping_view == KeyMappingView::Layout {
                                tabs::key_mapping::layout_move(&mut app, 1, 0);
                            } else if app.tab == 1 && app.depth_view_mode == DepthViewMode::BarChart {
                                let max_key = app.key_depths.len().min(66).saturating_sub(1);
                                if app.depth_cursor < max_key {
                                    app.depth_cursor += 1;
                                }
                            } else if app.tab == 0 {
                                let coarse = key.modifiers.contains(KeyModifiers::SHIFT);
                                match app.info_tags.get(app.selected).copied().unwrap_or(InfoTag::ReadOnly) {
                                    InfoTag::Profile => app.step_profile(1),
                                    InfoTag::Debounce => app.set_debounce(DEBOUNCE_SPINNER.increment_u8(app.info.debounce, coarse)),
                                    InfoTag::PollingRate => app.cycle_polling_rate(-1), // lower index = higher rate
                                    InfoTag::LedMode => app.set_led_mode((app.info.led_mode + 1).min(cmd::LED_MODE_MAX)),
                                    InfoTag::LedBrightness => app.set_brightness(BRIGHTNESS_SPINNER.increment_u8(app.info.led_brightness, coarse)),
                                    InfoTag::LedSpeed => {
                                        let current = speed_to_wire(app.info.led_speed);
                                        app.set_speed(SPEED_SPINNER.increment_u8(current, coarse));
                                    }
                                    InfoTag::LedRed => { let r = RGB_SPINNER.increment_u8(app.info.led_r, coarse); app.set_color(r, app.info.led_g, app.info.led_b); }
                                    InfoTag::LedGreen => { let g = RGB_SPINNER.increment_u8(app.info.led_g, coarse); app.set_color(app.info.led_r, g, app.info.led_b); }
                                    InfoTag::LedBlue => { let b = RGB_SPINNER.increment_u8(app.info.led_b, coarse); app.set_color(app.info.led_r, app.info.led_g, b); }
                                    InfoTag::LedDazzle => { app.toggle_dazzle(); tabs::audio::reapply_if_active(&mut app); }
                                    InfoTag::SideMode => app.set_side_mode((app.info.side_mode + 1).min(cmd::LED_MODE_MAX)),
                                    InfoTag::SideBrightness => app.set_side_brightness(BRIGHTNESS_SPINNER.increment_u8(app.info.side_brightness, coarse)),
                                    InfoTag::SideSpeed => {
                                        let current = speed_to_wire(app.info.side_speed);
                                        app.set_side_speed(SPEED_SPINNER.increment_u8(current, coarse));
                                    }
                                    InfoTag::SideRed => { let r = RGB_SPINNER.increment_u8(app.info.side_r, coarse); app.set_side_color(r, app.info.side_g, app.info.side_b); }
                                    InfoTag::SideGreen => { let g = RGB_SPINNER.increment_u8(app.info.side_g, coarse); app.set_side_color(app.info.side_r, g, app.info.side_b); }
                                    InfoTag::SideBlue => { let b = RGB_SPINNER.increment_u8(app.info.side_b, coarse); app.set_side_color(app.info.side_r, app.info.side_g, b); }
                                    InfoTag::SideDazzle => app.toggle_side_dazzle(),
                                    InfoTag::OsMode => { if let Some(ref opts) = app.options.clone() { app.set_os_mode(OS_MODE_SPINNER.increment_u8(opts.os_mode, coarse)); } }
                                    InfoTag::FnLayer => { if let Some(ref opts) = app.options.clone() { app.set_fn_layer(FN_LAYER_SPINNER.increment_u8(opts.fn_layer, coarse)); } }
                                    InfoTag::WasdSwap => app.toggle_wasd_swap(),
                                    InfoTag::AntiMistouch => app.toggle_anti_mistouch(),
                                    InfoTag::RtStability => { if let Some(ref opts) = app.options.clone() { app.set_rt_stability(RT_STABILITY_SPINNER.increment_u8(opts.rt_stability, coarse)); } }
                                    InfoTag::SleepIdleBt => { let step = if coarse { SLEEP_TIME_SPINNER.step_coarse } else { SLEEP_TIME_SPINNER.step } as i32; app.update_sleep_time(SleepField::IdleBt, step); }
                                    InfoTag::SleepIdle24g => { let step = if coarse { SLEEP_TIME_SPINNER.step_coarse } else { SLEEP_TIME_SPINNER.step } as i32; app.update_sleep_time(SleepField::Idle24g, step); }
                                    InfoTag::SleepDeepBt => { let step = if coarse { SLEEP_TIME_SPINNER.step_coarse } else { SLEEP_TIME_SPINNER.step } as i32; app.update_sleep_time(SleepField::DeepBt, step); }
                                    InfoTag::SleepDeep24g => { let step = if coarse { SLEEP_TIME_SPINNER.step_coarse } else { SLEEP_TIME_SPINNER.step } as i32; app.update_sleep_time(SleepField::Deep24g, step); }
                                    InfoTag::AudioDevice => tabs::audio::cycle_device(&mut app, 1),
                                    InfoTag::AudioVizStyle => tabs::audio::cycle_style(&mut app, 1),
                                    InfoTag::AudioRate => tabs::audio::cycle_rate(&mut app, 1),
                                    InfoTag::ScreenRate => tabs::screen::cycle_rate(&mut app, 1),
                                    InfoTag::ScreenRegion => tabs::screen::cycle_region(&mut app, 1),
                                    InfoTag::ScreenTestSwatch => tabs::screen::cycle_test_swatch(&mut app, 1),
                                    InfoTag::ScreenGainR => tabs::screen::adjust_calibration(&mut app, tabs::screen::CalField::GainR, 1, coarse),
                                    InfoTag::ScreenGainG => tabs::screen::adjust_calibration(&mut app, tabs::screen::CalField::GainG, 1, coarse),
                                    InfoTag::ScreenGainB => tabs::screen::adjust_calibration(&mut app, tabs::screen::CalField::GainB, 1, coarse),
                                    InfoTag::ScreenGammaR => tabs::screen::adjust_calibration(&mut app, tabs::screen::CalField::GammaR, 1, coarse),
                                    InfoTag::ScreenGammaG => tabs::screen::adjust_calibration(&mut app, tabs::screen::CalField::GammaG, 1, coarse),
                                    InfoTag::ScreenGammaB => tabs::screen::adjust_calibration(&mut app, tabs::screen::CalField::GammaB, 1, coarse),
                                    InfoTag::ScreenSaturation => tabs::screen::adjust_calibration(&mut app, tabs::screen::CalField::Saturation, 1, coarse),
                                    InfoTag::UserPicLayer => {
                                        let n = (app.userpic_layer + 1) % 4;
                                        app.set_userpic_layer(n);
                                    }
                                    _ => {}
                                }
                            }
                            #[cfg(feature = "notify")]
                            if app.tab == 3 {
                                handle_notify_input(&mut app, key.code);
                            }
                        }
                        KeyCode::Char('r') => {
                            // Re-check battery/idle state before refresh
                            if app.is_wireless {
                                app.refresh_battery();
                                app.refresh_dongle_status();
                            }
                            let is_idle = app.is_wireless && app.battery.as_ref().map(|b| b.idle).unwrap_or(false);
                            if is_idle {
                                app.status_msg = "Keyboard sleeping - press a key to wake before querying".to_string();
                            } else {
                                app.status_msg = "Refreshing...".to_string();
                                app.load_device_info();
                                app.load_options(); // Options are on tab 0
                                if app.tab == 2 { app.load_key_mapping(); app.load_triggers(); }
                            }
                        }
                        KeyCode::Enter if app.tab == 0 => {
                            match app.info_tags.get(app.selected).copied().unwrap_or(InfoTag::ReadOnly) {
                                InfoTag::Device => {
                                    app.scan_device_picker();
                                    app.show_device_picker = true;
                                }
                                InfoTag::FirmwareCheck => app.check_firmware(),
                                InfoTag::LedColorHex => app.start_hex_input(HexColorTarget::MainLed),
                                InfoTag::SideColorHex => app.start_hex_input(HexColorTarget::SideLed),
                                _ => {}
                            }
                        }
                        KeyCode::Char('#') if app.tab == 0 => {
                            match app.info_tags.get(app.selected).copied().unwrap_or(InfoTag::ReadOnly) {
                                InfoTag::LedRed | InfoTag::LedGreen | InfoTag::LedBlue | InfoTag::LedColorHex => {
                                    app.start_hex_input(HexColorTarget::MainLed);
                                }
                                InfoTag::SideRed | InfoTag::SideGreen | InfoTag::SideBlue | InfoTag::SideColorHex => {
                                    app.start_hex_input(HexColorTarget::SideLed);
                                }
                                _ => {}
                            }
                        }
                        KeyCode::Char(c) if app.tab == 0 && c.is_ascii_hexdigit() => {
                            match app.info_tags.get(app.selected).copied().unwrap_or(InfoTag::ReadOnly) {
                                InfoTag::LedColorHex => {
                                    app.start_hex_input(HexColorTarget::MainLed);
                                    app.hex_input.clear();
                                    app.hex_input.push(c.to_ascii_uppercase());
                                }
                                InfoTag::SideColorHex => {
                                    app.start_hex_input(HexColorTarget::SideLed);
                                    app.hex_input.clear();
                                    app.hex_input.push(c.to_ascii_uppercase());
                                }
                                _ => {}
                            }
                        }
                        KeyCode::Char('f') if app.tab == 2 => {
                            app.key_mapping_filter_open = true;
                            app.key_mapping_filter_field = 0;
                        }
                        KeyCode::Char('m') => {
                            app.depth_monitoring = !app.depth_monitoring;
                            if let Some(ref keyboard) = app.keyboard {
                                if app.depth_monitoring {
                                    let _ = keyboard.start_magnetism_report();
                                } else {
                                    let _ = keyboard.stop_magnetism_report();
                                }
                            }
                            app.status_msg = if app.depth_monitoring {
                                "Key depth monitoring ENABLED".to_string()
                            } else {
                                "Key depth monitoring DISABLED".to_string()
                            };
                        }
                        KeyCode::Char('c') => {
                            if let Err(e) = app.connect() {
                                app.status_msg = e;
                            } else {
                                // Skip loading if keyboard is sleeping
                                let is_idle = app.battery.as_ref().map(|b| b.idle).unwrap_or(false);
                                if !is_idle {
                                    app.load_device_info();
                                }
                            }
                        }
                        KeyCode::Char('d') => {
                            app.scan_device_picker();
                            app.show_device_picker = true;
                        }
                        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.cycle_profile();
                        }
                        KeyCode::Char('1') if key.modifiers.contains(KeyModifiers::CONTROL) => app.set_profile(Profile::ALL[0]),
                        KeyCode::Char('2') if key.modifiers.contains(KeyModifiers::CONTROL) => app.set_profile(Profile::ALL[1]),
                        KeyCode::Char('3') if key.modifiers.contains(KeyModifiers::CONTROL) => app.set_profile(Profile::ALL[2]),
                        KeyCode::Char('4') if key.modifiers.contains(KeyModifiers::CONTROL) => app.set_profile(Profile::ALL[3]),
                        KeyCode::PageUp if app.tab == 2 => {
                            for _ in 0..10 {
                                app.scroll_state.scroll_up();
                            }
                            app.key_mapping_selected = app.key_mapping_selected.saturating_sub(10);
                        }
                        KeyCode::PageDown if app.tab == 2 => {
                            let n = tabs::key_mapping::visible_indices(&app).len();
                            for _ in 0..10 {
                                app.scroll_state.scroll_down();
                            }
                            app.key_mapping_selected = (app.key_mapping_selected + 10).min(n.saturating_sub(1));
                        }
                        KeyCode::Char('p') if app.tab == 0 => app.apply_per_key_color(),
                        KeyCode::Char('v') if app.tab == 1 => app.toggle_depth_view(),
                        KeyCode::Char('v') if app.tab == 2 => {
                            app.key_mapping_view = match app.key_mapping_view {
                                KeyMappingView::List => KeyMappingView::Layout,
                                KeyMappingView::Layout => KeyMappingView::List,
                            };
                        }
                        KeyCode::Char('s') if app.tab == 2 => {
                            app.key_mapping_sort = app.key_mapping_sort.cycle();
                        }
                        // Key Mapping tab: Enter/'e' edit the selected key, 'g' edits all keys.
                        KeyCode::Enter | KeyCode::Char('e') if app.tab == 2 => {
                            let visible = tabs::key_mapping::visible_indices(&app);
                            if let Some(&ri) = visible.get(app.key_mapping_selected) {
                                let idx = usize::from(app.key_rows[ri].index);
                                app.open_trigger_edit_key(idx);
                            }
                        }
                        KeyCode::Char('g') if app.tab == 2 => app.open_trigger_edit_global(),
                        KeyCode::Char('x') if app.tab == 1 => app.clear_depth_data(),
                        KeyCode::Char(' ') if app.tab == 1 => {
                            if app.depth_view_mode == DepthViewMode::BarChart {
                                app.toggle_key_selection(app.depth_cursor);
                                let label = get_key_label(&app, app.depth_cursor);
                                if app.selected_keys.contains(&app.depth_cursor) {
                                    app.status_msg = format!("Selected Key {label} for time series");
                                } else {
                                    app.status_msg = format!("Deselected Key {label}");
                                }
                            }
                        }
                        // ── Notify tab input handling ──
                        #[cfg(feature = "notify")]
                        _ if app.tab == 3 => {
                            handle_notify_input(&mut app, key.code);
                        }
                        _ => {}
                    }
                } else if let Some(Ok(Event::Mouse(mouse))) = maybe_event {
                    // Handle mouse events
                    let pos = Position::new(mouse.column, mouse.row);
                    let tab_bar = app.tab_bar_area.get();
                    let content = app.content_area.get();

                    match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        // Check if click is in tab bar area
                        if tab_bar.contains(pos) {
                            // Calculate which tab was clicked
                            // Tabs render with border (1 char), then " Tab1 │ Tab2 │ ..."
                            let tab_names = app.tab_names();
                            let inner_x = mouse.column.saturating_sub(tab_bar.x + 1); // Account for border
                            let mut tab_pos = 1u16; // Initial padding
                            for (i, name) in tab_names.iter().enumerate() {
                                let tab_width = name.len() as u16;
                                if inner_x >= tab_pos && inner_x < tab_pos + tab_width {
                                    let old_tab = app.tab;
                                    app.tab = i;
                                    app.selected = 0;
                                    app.trigger_scroll = 0;
                                    app.scroll_state = ScrollViewState::new();
                                    app.auto_load_tab();
                                    if old_tab != app.tab {
                                        app.status_msg = format!("Switched to tab {}", i);
                                    }
                                    break;
                                }
                                tab_pos += tab_width + 3; // Tab width + " │ " separator
                            }
                        }

                        // Check if click is in content area
                        if content.contains(pos) {
                            // Row within content area (accounting for any border)
                            let content_row = (mouse.row.saturating_sub(content.y + 1)) as usize;
                            // Device Info — items in the list
                            if app.tab == 0 && content_row < app.info_tags.len() {
                                app.selected = content_row;
                            }
                        }
                    }
                    MouseEventKind::ScrollUp if content.contains(pos) => {
                        app.scroll_state.scroll_up();
                    }
                    MouseEventKind::ScrollDown if content.contains(pos) => {
                        app.scroll_state.scroll_down();
                    }
                    _ => {}
                    }
                } else if let Some(Ok(Event::Resize(_, _))) = maybe_event {
                    // Resize is handled automatically by ratatui on next draw
                }
            }

            // Handle keyboard EP2 events - low-latency channel from dedicated reader thread
            // This wakes immediately when events arrive (not tick-based)
            // Event coalescing: drain all pending events before redraw, keeping only latest depth per key
            result = async {
                match &mut app.event_rx {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                // Collect depth events for coalescing (key_index -> (timestamp, depth_raw))
                let mut pending_depths: HashMap<u8, (f64, u16)> = HashMap::new();
                // Collect non-depth events to process after draining
                let mut other_events: Vec<(f64, VendorEvent)> = Vec::new();
                let mut channel_closed = false;

                match result {
                    Ok(ts) => {
                        if let VendorEvent::KeyDepth { key_index, depth_raw } = ts.event {
                            pending_depths.insert(key_index, (ts.timestamp, depth_raw));
                        } else {
                            other_events.push((ts.timestamp, ts.event));
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::debug!("Event receiver lagged by {} events", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::debug!("Event channel closed");
                        channel_closed = true;
                    }
                }

                // Drain remaining events without blocking (coalesce depth events by key)
                if !channel_closed
                    && let Some(ref mut rx) = app.event_rx {
                        loop {
                            match rx.try_recv() {
                                Ok(ts) => {
                                    if let VendorEvent::KeyDepth { key_index, depth_raw } = ts.event {
                                        // Keep only latest depth per key
                                        pending_depths.insert(key_index, (ts.timestamp, depth_raw));
                                    } else {
                                        other_events.push((ts.timestamp, ts.event));
                                    }
                                }
                                Err(broadcast::error::TryRecvError::Empty) => break,
                                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                                    tracing::debug!("Event receiver lagged by {} events (drain)", n);
                                }
                                Err(broadcast::error::TryRecvError::Closed) => {
                                    channel_closed = true;
                                    break;
                                }
                            }
                        }
                    }

                if channel_closed {
                    app.event_rx = None;
                }

                // Process non-depth events first (order preserved)
                for (timestamp, event) in other_events {
                    app.handle_vendor_notification(timestamp, event);
                }

                // Process coalesced depth events (one per key)
                for (key_index, (timestamp, depth_raw)) in pending_depths {
                    app.handle_depth_event(key_index, depth_raw, timestamp);
                }
            }

            // Handle tick updates
            _ = tick_interval.tick() => {
                // Start/stop audio/screen-reactive to follow the LED mode.
                tabs::audio::reconcile(&mut app);
                tabs::screen::reconcile(&mut app);

                // Fast tick (~60fps) when an animated panel needs it: the notify
                // tab, or the audio/screen preview (Device Info tab while running).
                let want_ms = {
                    let mut fast =
                        app.tab == 0 && (app.audio.is_running() || app.screen.is_running());
                    #[cfg(feature = "notify")]
                    { fast = fast || app.tab == 3; }
                    if fast { 16 } else { 100 }
                };
                if want_ms != last_tick_ms {
                    last_tick_ms = want_ms;
                    tick_interval = tokio::time::interval(Duration::from_millis(want_ms));
                    tick_interval.reset();
                }

                // Advance spinner animation
                app.throbber_state.calc_next();

                // Read depth reports (handles stale key cleanup internally)
                app.read_input_reports();

                // Notify tab tick (preview animation + D-Bus poll)
                #[cfg(feature = "notify")]
                app.notify_tick();

                // Poll animation engine status (on notify tab, or when overlay active)
                app.poll_anim_status();

                // Refresh battery every 30 seconds for wireless devices
                if app.is_wireless && app.last_battery_check.elapsed() >= Duration::from_secs(30) {
                    let was_idle = app.battery.as_ref().map(|b| b.idle).unwrap_or(false);
                    app.refresh_battery();
                    app.refresh_dongle_status();
                    let is_idle = app.battery.as_ref().map(|b| b.idle).unwrap_or(false);

                    if is_idle {
                        app.status_msg = "Keyboard sleeping - press a key to wake before querying".to_string();
                    } else if was_idle {
                        // Keyboard just woke up - load device info now
                        app.status_msg = "Keyboard awake - loading settings...".to_string();
                        app.load_device_info();
                    }
                }
            }
        }
    }

    // Stop reactive modes while the alternate screen is still active, so they
    // release the keyboard before the terminal is restored.
    tabs::audio::shutdown_async(&mut app).await;
    tabs::screen::shutdown_async(&mut app).await;

    // If we quit while a reactive mode was active, return the keyboard to the
    // mode selected beforehand — `send_main_led` resends the user's brightness/
    // speed/color, so their normal lighting is restored intact.
    if let Some(prev) = app.reactive_prev_mode.take() {
        app.info.led_mode = prev;
        app.send_main_led();
    }

    // Cleanup - stop magnetism reporting and clear all animations
    if let Some(ref keyboard) = app.keyboard {
        if app.depth_monitoring {
            let _ = keyboard.stop_magnetism_report();
        }
        let _ = keyboard.anim_clear();
    }
    disable_raw_mode()?;
    stdout()
        .execute(DisableMouseCapture)?
        .execute(LeaveAlternateScreen)?;
    Ok(())
}

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title
            Constraint::Length(1), // Tabs
            Constraint::Min(10),   // Content
            Constraint::Length(1), // Status bar
        ])
        .split(f.area());

    // Title - show device name if connected, otherwise generic title
    let title_text = if app.connected && !app.device_name.is_empty() {
        format!("{} - Configuration Tool", app.device_name)
    } else {
        "MonsGeek/Akko Keyboard - Configuration Tool".to_string()
    };
    let profile_text = if app.connected {
        format!(" Profile {}/4  ^P ", app.info.profile.get() + 1)
    } else {
        String::new()
    };
    let title_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(10),
            Constraint::Length(profile_text.len() as u16),
        ])
        .split(chunks[0]);
    let title = Paragraph::new(title_text)
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center);
    f.render_widget(title, title_row[0]);
    f.render_widget(
        Paragraph::new(profile_text)
            .style(Style::default().fg(Color::Magenta))
            .alignment(Alignment::Right),
        title_row[1],
    );

    // Tabs, with a right-aligned navigation hint.
    let tab_hint = format!(" Alt+1-{}: switch tabs ", app.tab_count());
    let tab_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(10),
            Constraint::Length(tab_hint.len() as u16),
        ])
        .split(chunks[1]);
    let tabs = Tabs::new(app.tab_names())
        .select(app.tab)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .divider("│");
    f.render_widget(tabs, tab_row[0]);
    f.render_widget(
        Paragraph::new(tab_hint)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Right),
        tab_row[1],
    );

    // Store areas for mouse hit testing (using interior mutability)
    app.tab_bar_area.set(tab_row[0]);
    app.content_area.set(chunks[2]);

    // Content based on tab
    match app.tab {
        0 => render_device_info(f, app, chunks[2]),
        1 => render_depth_monitor(f, app, chunks[2]),
        2 => tabs::key_mapping::render_key_mapping(f, app, chunks[2]),
        #[cfg(feature = "notify")]
        3 => render_notify(f, app, chunks[2]),
        _ => {}
    }

    // Status bar
    let status_color = if app.connected {
        Color::Green
    } else {
        Color::Red
    };
    let conn_status = if app.connected {
        "Connected"
    } else {
        "Disconnected"
    };
    let profile_str = if app.connected {
        format!(" Profile {}", app.info.profile.get() + 1)
    } else {
        String::new()
    };

    // Battery status for wireless devices
    let battery_str = if app.is_wireless {
        if let Some(ref batt) = app.battery {
            let icon = if batt.charging {
                "⚡"
            } else if batt.level > 75 {
                "█"
            } else if batt.level > 50 {
                "▆"
            } else if batt.level > 25 {
                "▃"
            } else {
                "▁"
            };
            // Show source indicator: (k)ernel or (v)endor
            let src = match &app.battery_source {
                Some(BatterySource::Kernel(_)) => "k",
                Some(BatterySource::Vendor) => "v",
                None => "?",
            };
            // Show idle indicator when keyboard is sleeping
            let idle_str = if batt.idle { " zzz" } else { "" };
            format!(" {}{}%({src}){idle_str}", icon, batt.level)
        } else {
            " ?%".to_string()
        }
    } else {
        String::new()
    };

    let monitoring_str = if app.depth_monitoring {
        " MONITORING"
    } else {
        ""
    };

    let status_text = if app.hex_editing {
        format!(
            " [{}{}{}] Enter hex color: #{} | Esc:Cancel Enter:Apply",
            conn_status, profile_str, battery_str, app.hex_input
        )
    } else {
        format!(
            " [{}{}{}] {} | ?:Help q:Quit{}",
            conn_status, profile_str, battery_str, app.status_msg, monitoring_str
        )
    };
    let status = Paragraph::new(status_text).style(Style::default().fg(status_color));
    f.render_widget(status, chunks[3]);

    // Help popup (renders on top)
    if app.show_help {
        render_help_popup(f, f.area());
    }

    // Device picker popup (renders on top)
    if app.show_device_picker {
        render_device_picker(f, app, f.area());
    }

    // Trigger edit modal (renders on top)
    if app.trigger_edit_modal.is_some() {
        render_trigger_edit_modal(f, app, f.area());
    }

    // Key Mapping filter popup (renders on top)
    if app.key_mapping_filter_open {
        tabs::key_mapping::render_key_mapping_filter(f, app, f.area());
    }
}

/// Render device picker popup
fn render_device_picker(f: &mut Frame, app: &App, area: Rect) {
    let item_count = app.device_picker_items.len();
    // Size: fixed width, height based on items (min 3 for empty message)
    let popup_height = (item_count as u16 + 4).clamp(5, area.height.saturating_sub(4));
    let popup_width = 60.min(area.width.saturating_sub(4));
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    if item_count == 0 {
        let msg = Paragraph::new("No devices found.")
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .title(" Device Picker ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );
        f.render_widget(msg, popup_area);
        return;
    }

    let items: Vec<ListItem> = app
        .device_picker_items
        .iter()
        .enumerate()
        .map(|(i, (probed, label))| {
            let connected_marker = if !probed.responsive { " (asleep)" } else { "" };
            let version_str = label
                .version
                .map(|v| format!(" v{}.{:02}", v / 100, v % 100))
                .unwrap_or_default();
            let text = format!(
                "#{:<2} {:<22} {:<7} [{}{}]{}",
                label.index,
                label.model_name,
                label.transport_name,
                label.device_id.unwrap_or(0),
                version_str,
                connected_marker,
            );
            let style = if i == app.device_picker_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(text).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" Device Picker — Enter to connect, Esc to cancel ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(list, popup_area);
}

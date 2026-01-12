// Audio Reactive LED Mode
// Captures system audio and maps frequency spectrum to keyboard RGB colors

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use spectrum_analyzer::scaling::divide_by_N_sqrt;
use spectrum_analyzer::windows::hann_window;
use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::hid::MonsGeekDevice;
use crate::protocol::cmd;

/// Number of frequency bands to analyze
const NUM_BANDS: usize = 8;

/// FFT sample size (must be power of 2)
const FFT_SIZE: usize = 1024;

/// Target FPS for RGB updates (limited by HID bandwidth - 7 pages × ~13ms = ~91ms/frame max)
const TARGET_FPS: u32 = 10;

/// Audio reactive state shared between threads
pub struct AudioState {
    /// Current frequency band magnitudes (0.0 - 1.0)
    pub bands: Mutex<[f32; NUM_BANDS]>,
    /// Peak values for decay animation
    pub peaks: Mutex<[f32; NUM_BANDS]>,
    /// Running flag
    pub running: AtomicBool,
    /// Sample rate from audio device
    pub sample_rate: AtomicU32,
}

impl Default for AudioState {
    fn default() -> Self {
        Self {
            bands: Mutex::new([0.0; NUM_BANDS]),
            peaks: Mutex::new([0.0; NUM_BANDS]),
            running: AtomicBool::new(false),
            sample_rate: AtomicU32::new(44100),
        }
    }
}

impl AudioState {
    /// Get a copy of current bands
    pub fn get_bands(&self) -> [f32; NUM_BANDS] {
        *self.bands.lock().unwrap()
    }

    /// Update bands with new values
    pub fn set_bands(&self, new_bands: [f32; NUM_BANDS]) {
        *self.bands.lock().unwrap() = new_bands;
    }

    /// Check if running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Stop the audio capture
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// Audio capture context - holds the stream and shared state
pub struct AudioCapture {
    pub state: Arc<AudioState>,
    _sample_buffer: Arc<Mutex<Vec<f32>>>,
    _sample_rate: u32,
    // Note: stream is stored as Box<dyn Any> because cpal::Stream is not Send
    // It must be dropped on the same thread it was created
    _stream: Box<dyn std::any::Any>,
}

impl AudioCapture {
    /// Start audio capture
    pub fn start(config: AudioConfig) -> Result<Self, String> {
        let state = Arc::new(AudioState::default());
        let host = cpal::default_host();

        // Auto-detect monitor source for system audio
        if let Ok(monitor) = get_pulseaudio_monitor() {
            std::env::set_var("PULSE_SOURCE", &monitor);
        }

        // Suppress ALSA warnings about unavailable plugins (mostly works)
        std::env::set_var("ALSA_DEBUG", "0");

        // Suppress libasound stderr output - these warnings are harmless
        // Note: ALSA enumeration will still print some warnings to stderr
        eprintln!("(Ignoring ALSA warnings below - they're harmless)");

        let audio_device = find_audio_device(&host)?;
        let audio_config = audio_device
            .default_input_config()
            .map_err(|e| format!("Failed to get audio config: {e}"))?;

        let sample_rate = audio_config.sample_rate().0;
        state.sample_rate.store(sample_rate, Ordering::SeqCst);

        let sample_buffer: Arc<Mutex<Vec<f32>>> =
            Arc::new(Mutex::new(Vec::with_capacity(FFT_SIZE * 2)));
        let sample_buffer_clone = Arc::clone(&sample_buffer);

        let stream = audio_device
            .build_input_stream(
                &audio_config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut buffer) = sample_buffer_clone.lock() {
                        buffer.extend_from_slice(data);
                        if buffer.len() > FFT_SIZE * 4 {
                            let drain_to = buffer.len() - FFT_SIZE * 2;
                            buffer.drain(..drain_to);
                        }
                    }
                },
                |err| {
                    eprintln!("Audio stream error: {err}");
                },
                None,
            )
            .map_err(|e| format!("Failed to build audio stream: {e}"))?;

        stream
            .play()
            .map_err(|e| format!("Failed to start audio stream: {e}"))?;
        state.running.store(true, Ordering::SeqCst);

        // Start FFT processing thread
        let state_clone = Arc::clone(&state);
        let buffer_clone = Arc::clone(&sample_buffer);
        thread::spawn(move || {
            let mut smoothed_bands = [0.0f32; NUM_BANDS];
            let process_interval = Duration::from_millis(16);
            let mut loop_count = 0u32;

            while state_clone.running.load(Ordering::SeqCst) {
                let start = Instant::now();
                loop_count += 1;

                let (samples, buf_len): (Vec<f32>, usize) = {
                    if let Ok(buffer) = buffer_clone.lock() {
                        let len = buffer.len();
                        if len >= FFT_SIZE {
                            (buffer[len - FFT_SIZE..].to_vec(), len)
                        } else {
                            (vec![0.0; FFT_SIZE], len)
                        }
                    } else {
                        (vec![0.0; FFT_SIZE], 0)
                    }
                };

                // Debug: check audio data every 5 seconds (300 loops at ~60fps)
                if loop_count.is_multiple_of(300) && std::env::var("RUST_LOG").is_ok() {
                    let max_sample = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
                    eprintln!("[Audio] buf={buf_len}, peak={max_sample:.3}");
                }

                let raw_bands = analyze_spectrum(&samples, sample_rate);

                for i in 0..NUM_BANDS {
                    smoothed_bands[i] = smoothed_bands[i] * config.smoothing
                        + raw_bands[i] * (1.0 - config.smoothing);
                }

                state_clone.set_bands(smoothed_bands);

                let elapsed = start.elapsed();
                if elapsed < process_interval {
                    thread::sleep(process_interval - elapsed);
                }
            }
        });

        Ok(Self {
            state,
            _sample_buffer: sample_buffer,
            _sample_rate: sample_rate,
            _stream: Box::new(stream),
        })
    }

    /// Stop audio capture
    pub fn stop(&self) {
        self.state.stop();
    }

    /// Get current spectrum bands
    pub fn get_bands(&self) -> [f32; NUM_BANDS] {
        self.state.get_bands()
    }
}

/// Audio reactive mode configuration
#[derive(Clone)]
pub struct AudioConfig {
    /// Color mode: "spectrum" (rainbow), "solid" (single color pulse), "gradient"
    pub color_mode: String,
    /// Base hue for solid mode (0-360)
    pub base_hue: f32,
    /// Sensitivity multiplier (0.5 - 2.0)
    pub sensitivity: f32,
    /// Smoothing factor (0.0 = instant, 0.9 = very smooth)
    pub smoothing: f32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            color_mode: "spectrum".to_string(),
            base_hue: 0.0,
            sensitivity: 1.0,
            smoothing: 0.3,
        }
    }
}

use crate::color::hsv_to_rgb;

/// Map frequency bands to per-key RGB colors
fn bands_to_colors(
    bands: &[f32; NUM_BANDS],
    key_count: usize,
    config: &AudioConfig,
) -> Vec<(u8, u8, u8)> {
    let mut colors = Vec::with_capacity(key_count);

    // Calculate average energy for overall brightness
    let avg_energy: f32 = bands.iter().sum::<f32>() / NUM_BANDS as f32;

    for i in 0..key_count {
        // Map key position to a frequency band
        let band_idx = (i * NUM_BANDS / key_count).min(NUM_BANDS - 1);
        let band_value = bands[band_idx] * config.sensitivity;
        let intensity = band_value.min(1.0);

        let (r, g, b) = match config.color_mode.as_str() {
            "solid" => {
                // Pulse single color based on overall energy
                let v = (avg_energy * config.sensitivity).min(1.0);
                hsv_to_rgb(config.base_hue, 1.0, v)
            }
            "gradient" => {
                // Gradient from base_hue, intensity affects saturation
                let hue = (config.base_hue + (i as f32 * 360.0 / key_count as f32)) % 360.0;
                hsv_to_rgb(hue, 0.5 + intensity * 0.5, intensity)
            }
            _ => {
                // "spectrum" - rainbow colors mapped to frequency bands
                let hue = (band_idx as f32 * 360.0 / NUM_BANDS as f32) % 360.0;
                hsv_to_rgb(hue, 1.0, intensity)
            }
        };

        colors.push((r, g, b));
    }

    colors
}

/// Analyze audio samples and extract frequency bands
fn analyze_spectrum(samples: &[f32], sample_rate: u32) -> [f32; NUM_BANDS] {
    let mut bands = [0.0f32; NUM_BANDS];

    if samples.len() < FFT_SIZE {
        return bands;
    }

    // Check if there's any audio at all
    let max_sample = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    if max_sample < 0.001 {
        // Silence - return zeros
        return bands;
    }

    // Apply Hann window
    let windowed: Vec<f32> = hann_window(&samples[..FFT_SIZE]).to_vec();

    // Max frequency is half the sample rate (Nyquist)
    let max_freq = (sample_rate / 2) as f32;
    let freq_limit = FrequencyLimit::Range(20.0, max_freq.min(20000.0));

    // Compute FFT spectrum
    let spectrum = match samples_fft_to_spectrum(
        &windowed,
        sample_rate,
        freq_limit,
        Some(&divide_by_N_sqrt),
    ) {
        Ok(s) => s,
        Err(_) => return bands,
    };

    // Frequency band ranges (Hz) - logarithmic scale for better visualization
    // Each band also has a weight to compensate for frequency distribution
    let band_ranges = [
        (20.0, 60.0, 2.0),       // Sub-bass (boost - fewer bins)
        (60.0, 150.0, 1.5),      // Bass
        (150.0, 400.0, 1.2),     // Low-mids
        (400.0, 1000.0, 1.0),    // Mids (reference)
        (1000.0, 2500.0, 1.0),   // Upper-mids
        (2500.0, 6000.0, 1.2),   // Presence
        (6000.0, 12000.0, 1.5),  // Brilliance
        (12000.0, 20000.0, 2.0), // Air (boost - often quiet)
    ];

    // Count bins in each band for proper averaging
    let mut band_counts = [0u32; NUM_BANDS];

    // Sum magnitudes in each band
    for (freq, magnitude) in spectrum.data().iter() {
        let freq_hz = freq.val();
        for (band_idx, (low, high, _weight)) in band_ranges.iter().enumerate() {
            if freq_hz >= *low && freq_hz < *high {
                bands[band_idx] += magnitude.val();
                band_counts[band_idx] += 1;
            }
        }
    }

    // Average and normalize each band
    for (i, band) in bands.iter_mut().enumerate() {
        if band_counts[i] > 0 {
            // Average the magnitudes
            *band /= band_counts[i] as f32;
            // Apply band weight
            *band *= band_ranges[i].2;
        }
    }

    // Find max band value for dynamic normalization
    let max_band = bands.iter().fold(0.0f32, |a, b| f32::max(a, *b));

    // Minimum threshold for "silence" detection
    // Below this threshold, treat as silence (dark)
    const MIN_THRESHOLD: f32 = 0.0005;

    if max_band < MIN_THRESHOLD {
        // Silence - return zeros
        return [0.0; NUM_BANDS];
    }

    // Normalize to 0-1 range with dynamic range compression
    // Use a reference level to make quiet audio still visible
    let reference_level = max_band.max(0.01); // Minimum reference to avoid over-amplification

    for band in bands.iter_mut() {
        // Normalize against reference level
        let normalized = *band / reference_level;
        // Power curve for more dynamic range (0.5 = square root = more visible low values)
        *band = normalized.powf(0.5).min(1.0);
    }

    bands
}

/// List available audio input devices
pub fn list_audio_devices() -> Vec<String> {
    let host = cpal::default_host();
    let mut devices = Vec::new();

    // Try to find loopback/monitor devices first
    if let Ok(input_devices) = host.input_devices() {
        for device in input_devices {
            if let Ok(name) = device.name() {
                devices.push(name);
            }
        }
    }

    devices
}

/// Run audio reactive mode (blocking)
/// This starts audio capture in a background thread and runs the RGB update loop
pub fn run_audio_reactive(
    device: &MonsGeekDevice,
    config: AudioConfig,
    running: Arc<AtomicBool>,
) -> Result<(), String> {
    println!("Starting audio capture...");

    // Start audio capture (creates stream and FFT processing thread)
    let audio_capture = AudioCapture::start(config.clone())?;

    println!("Audio capture started, setting LED mode...");

    // Set LED mode to per-key colors (LightUserPicture with layer 0)
    device.set_led_with_option(cmd::LedMode::UserPicture.as_u8(), 4, 0, 0, 0, 0, false, 0);
    thread::sleep(Duration::from_millis(200));

    // Run the RGB rendering loop
    run_rgb_loop(device, &audio_capture.state, &config, running)?;

    // Stop audio capture
    audio_capture.stop();

    println!("Audio reactive mode stopped");
    Ok(())
}

/// RGB rendering loop - reads from AudioState and sends colors to keyboard
pub fn run_rgb_loop(
    device: &MonsGeekDevice,
    audio_state: &Arc<AudioState>,
    config: &AudioConfig,
    running: Arc<AtomicBool>,
) -> Result<(), String> {
    let key_count = device.key_count() as usize;
    let frame_duration = Duration::from_millis(1000 / TARGET_FPS as u64);
    let mut frame_count = 0u32;

    running.store(true, Ordering::SeqCst);

    while running.load(Ordering::SeqCst) && audio_state.is_running() {
        let frame_start = Instant::now();

        // Get current spectrum from audio thread
        let bands = audio_state.get_bands();

        // Generate colors
        let colors = bands_to_colors(&bands, key_count, config);

        // Debug output every 5 seconds (only if RUST_LOG is set)
        frame_count += 1;
        if frame_count.is_multiple_of(TARGET_FPS * 5) && std::env::var("RUST_LOG").is_ok() {
            let avg: f32 = bands.iter().sum::<f32>() / NUM_BANDS as f32;
            let first_color = if colors.is_empty() {
                (0, 0, 0)
            } else {
                colors[0]
            };
            eprintln!(
                "[RGB] avg={:.2} bass={:.2} color0=({},{},{})",
                avg, bands[1], first_color.0, first_color.1, first_color.2
            );
        }

        // Send to keyboard (10ms page delay, 3ms inter-page = ~88ms per frame = ~11fps)
        device.set_per_key_colors_fast(&colors, 10, 3);

        // Maintain target FPS
        let elapsed = frame_start.elapsed();
        if elapsed < frame_duration {
            thread::sleep(frame_duration - elapsed);
        }
    }

    Ok(())
}

/// Find the best audio device for capture
fn find_audio_device(host: &cpal::Host) -> Result<cpal::Device, String> {
    // On Linux with PipeWire/PulseAudio, try to use the monitor source
    // This is done by setting PULSE_SOURCE environment variable
    if let Ok(monitor) = get_pulseaudio_monitor() {
        tracing::info!("Setting PULSE_SOURCE={}", monitor);
        std::env::set_var("PULSE_SOURCE", &monitor);
    }

    // Try to find monitor/loopback devices in cpal's device list
    if let Ok(devices) = host.input_devices() {
        for device in devices {
            if let Ok(name) = device.name() {
                let name_lower = name.to_lowercase();
                // PulseAudio/PipeWire monitor sources
                if name_lower.contains("monitor") || name_lower.contains("loopback") {
                    tracing::info!("Found monitor device: {}", name);
                    return Ok(device);
                }
            }
        }
    }

    // Try "pulse" device first (PulseAudio/PipeWire with PULSE_SOURCE set)
    if let Ok(devices) = host.input_devices() {
        for device in devices {
            if let Ok(name) = device.name() {
                if name == "pulse" || name == "pipewire" {
                    tracing::info!("Using {} device with monitor source", name);
                    return Ok(device);
                }
            }
        }
    }

    // Fall back to default input device
    host.default_input_device()
        .ok_or_else(|| "No audio input device found".to_string())
}

/// Get the PulseAudio/PipeWire monitor source name
fn get_pulseaudio_monitor() -> Result<String, String> {
    // Run: pactl list sources short | grep monitor
    let output = std::process::Command::new("pactl")
        .args(["list", "sources", "short"])
        .output()
        .map_err(|e| format!("Failed to run pactl: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            let source_name = parts[1];
            if source_name.contains(".monitor") {
                return Ok(source_name.to_string());
            }
        }
    }

    Err("No monitor source found".to_string())
}

/// Run a simple rainbow animation to test RGB without audio
pub fn run_rainbow_test(device: &MonsGeekDevice, running: Arc<AtomicBool>) -> Result<(), String> {
    println!("Starting rainbow test mode...");

    // Set LED mode to per-key colors (LightUserPicture with layer 0)
    device.set_led_with_option(cmd::LedMode::UserPicture.as_u8(), 4, 0, 0, 0, 0, false, 0);
    std::thread::sleep(Duration::from_millis(200));

    let key_count = device.key_count() as usize;
    let frame_duration = Duration::from_millis(1000 / 30); // 30 FPS
    let mut hue_offset = 0.0f32;

    running.store(true, Ordering::SeqCst);

    while running.load(Ordering::SeqCst) {
        let frame_start = Instant::now();

        // Generate rainbow colors across keys
        let mut colors = Vec::with_capacity(key_count);
        for i in 0..key_count {
            let hue = (hue_offset + (i as f32 * 360.0 / key_count as f32)) % 360.0;
            colors.push(hsv_to_rgb(hue, 1.0, 1.0));
        }

        device.set_per_key_colors_fast(&colors, 10, 5);

        hue_offset = (hue_offset + 5.0) % 360.0;

        let elapsed = frame_start.elapsed();
        if elapsed < frame_duration {
            std::thread::sleep(frame_duration - elapsed);
        }
    }

    println!("Rainbow test stopped");
    Ok(())
}

/// Simple test function to verify audio capture works
pub fn test_audio_capture() -> Result<(), String> {
    let host = cpal::default_host();
    let device = find_audio_device(&host)?;
    let name = device.name().unwrap_or_else(|_| "Unknown".to_string());

    println!("Audio device: {name}");

    let config = device
        .default_input_config()
        .map_err(|e| format!("Config error: {e}"))?;

    println!("Sample rate: {} Hz", config.sample_rate().0);
    println!("Channels: {}", config.channels());
    println!("Sample format: {:?}", config.sample_format());

    Ok(())
}

/// Test audio capture by printing audio levels for a few seconds
pub fn test_audio_levels() -> Result<(), String> {
    use std::io::Write;

    let host = cpal::default_host();

    // Auto-detect monitor source
    if let Ok(monitor) = get_pulseaudio_monitor() {
        println!("Found monitor source: {monitor}");
        std::env::set_var("PULSE_SOURCE", &monitor);
    } else {
        println!("No monitor source found, using default input");
    }

    let device = find_audio_device(&host)?;
    let name = device.name().unwrap_or_else(|_| "Unknown".to_string());
    println!("Using device: {name}");

    let config = device
        .default_input_config()
        .map_err(|e| format!("Config error: {e}"))?;

    let sample_rate = config.sample_rate().0;
    println!(
        "Sample rate: {} Hz, channels: {}",
        sample_rate,
        config.channels()
    );

    let callback_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let callback_count_clone = Arc::clone(&callback_count);
    let max_sample = Arc::new(Mutex::new(0.0f32));
    let max_sample_clone = Arc::clone(&max_sample);

    let stream = device
        .build_input_stream(
            &config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                callback_count_clone.fetch_add(1, Ordering::Relaxed);
                // Find max absolute sample value
                let local_max = data.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
                if let Ok(mut max) = max_sample_clone.lock() {
                    if local_max > *max {
                        *max = local_max;
                    }
                }
            },
            |err| {
                eprintln!("Audio error: {err}");
            },
            None,
        )
        .map_err(|e| format!("Failed to build stream: {e}"))?;

    stream.play().map_err(|e| format!("Failed to play: {e}"))?;

    println!("\nListening for 5 seconds...");
    for i in 0..5 {
        std::thread::sleep(Duration::from_secs(1));
        let callbacks = callback_count.load(Ordering::Relaxed);
        let peak = *max_sample.lock().unwrap();
        print!(
            "  Second {}: {} callbacks, peak: {:.4}",
            i + 1,
            callbacks,
            peak
        );
        // Visual level meter
        let bars = (peak * 50.0).min(50.0) as usize;
        print!(" [");
        for _ in 0..bars {
            print!("#");
        }
        for _ in bars..50 {
            print!(" ");
        }
        println!("]");
        std::io::stdout().flush().ok();

        // Reset peak for next second
        *max_sample.lock().unwrap() = 0.0;
    }

    drop(stream);
    println!(
        "\nDone. Total callbacks: {}",
        callback_count.load(Ordering::Relaxed)
    );
    Ok(())
}

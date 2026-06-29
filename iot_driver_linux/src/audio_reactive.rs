// Audio Reactive LED Mode
// Captures system audio and maps frequency spectrum to keyboard RGB colors

use spectrum_analyzer::scaling::divide_by_N_sqrt;
use spectrum_analyzer::windows::hann_window;
use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::protocol::{audio_viz, cmd};
use crate::pulse;
use monsgeek_keyboard::KeyboardInterface;

/// Number of frequency bands to analyze
const NUM_BANDS: usize = 8;

/// FFT sample size (must be power of 2)
const FFT_SIZE: usize = 1024;

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

/// Audio capture context — owns the capture + FFT threads and shared state.
pub struct AudioCapture {
    pub state: Arc<AudioState>,
    /// Human label of the resolved capture source (for callers to display).
    pub source_label: String,
    capture_thread: Option<thread::JoinHandle<()>>,
    fft_thread: Option<thread::JoinHandle<()>>,
}

impl AudioCapture {
    /// Start capturing from the resolved PulseAudio source.
    ///
    /// Spawns a capture thread (blocking PulseAudio reads → ring buffer) and an
    /// FFT thread (ring buffer → smoothed bands). Both stop on [`Self::stop`] or
    /// when the capture is dropped.
    pub fn start(config: AudioConfig) -> Result<Self, String> {
        let state = Arc::new(AudioState::default());

        let source = pulse::resolve_source(config.device.as_deref())?;
        let simple = pulse::open_record(&source.name)?;
        state
            .sample_rate
            .store(pulse::SAMPLE_RATE, Ordering::SeqCst);
        let source_label = source.label();

        let sample_buffer: Arc<Mutex<Vec<f32>>> =
            Arc::new(Mutex::new(Vec::with_capacity(FFT_SIZE * 2)));
        state.running.store(true, Ordering::SeqCst);

        // Capture thread: blocking PulseAudio reads → ring buffer.
        let capture_state = Arc::clone(&state);
        let capture_buffer = Arc::clone(&sample_buffer);
        let capture_thread = thread::spawn(move || {
            // ~6 ms of audio per read: low latency while still polling `running`.
            const READ_SAMPLES: usize = 256;
            let mut byte_buf = vec![0u8; READ_SAMPLES * 4];
            while capture_state.running.load(Ordering::SeqCst) {
                if simple.read(&mut byte_buf).is_err() {
                    break;
                }
                if let Ok(mut buffer) = capture_buffer.lock() {
                    buffer.extend(
                        byte_buf
                            .chunks_exact(4)
                            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])),
                    );
                    if buffer.len() > FFT_SIZE * 4 {
                        let drain_to = buffer.len() - FFT_SIZE * 2;
                        buffer.drain(..drain_to);
                    }
                }
            }
        });

        // FFT thread: ring buffer → smoothed bands.
        let smoothing = config.smoothing;
        let fft_state = Arc::clone(&state);
        let fft_buffer = Arc::clone(&sample_buffer);
        let fft_thread = thread::spawn(move || {
            let mut smoothed_bands = [0.0f32; NUM_BANDS];
            let process_interval = Duration::from_millis(16);
            let mut loop_count = 0u32;

            while fft_state.running.load(Ordering::SeqCst) {
                let start = Instant::now();
                loop_count += 1;

                let (samples, buf_len): (Vec<f32>, usize) = match fft_buffer.lock() {
                    Ok(buffer) => {
                        let len = buffer.len();
                        if len >= FFT_SIZE {
                            (buffer[len - FFT_SIZE..].to_vec(), len)
                        } else {
                            (vec![0.0; FFT_SIZE], len)
                        }
                    }
                    Err(_) => (vec![0.0; FFT_SIZE], 0),
                };

                // Debug: check audio data every ~5 seconds
                if loop_count.is_multiple_of(300) && std::env::var("RUST_LOG").is_ok() {
                    let max_sample = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
                    eprintln!("[Audio] buf={buf_len}, peak={max_sample:.3}");
                }

                let raw_bands = analyze_spectrum(&samples, pulse::SAMPLE_RATE);
                for i in 0..NUM_BANDS {
                    smoothed_bands[i] =
                        smoothed_bands[i] * smoothing + raw_bands[i] * (1.0 - smoothing);
                }
                fft_state.set_bands(smoothed_bands);

                let elapsed = start.elapsed();
                if elapsed < process_interval {
                    thread::sleep(process_interval - elapsed);
                }
            }
        });

        Ok(Self {
            state,
            source_label,
            capture_thread: Some(capture_thread),
            fft_thread: Some(fft_thread),
        })
    }

    /// Signal the capture + FFT threads to stop (non-blocking).
    pub fn stop(&self) {
        self.state.stop();
    }

    /// Get current spectrum bands.
    pub fn get_bands(&self) -> [f32; NUM_BANDS] {
        self.state.get_bands()
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.state.stop();
        if let Some(h) = self.capture_thread.take() {
            let _ = h.join();
        }
        if let Some(h) = self.fft_thread.take() {
            let _ = h.join();
        }
    }
}

/// Audio reactive mode configuration
#[derive(Clone)]
pub struct AudioConfig {
    /// Music-visualizer LED mode byte: MusicBars (22) or MusicPatterns (20).
    /// The keyboard renders the bars on-device; we only stream band levels.
    pub led_mode: u8,
    /// Style variant within the mode (MusicBars: 0-2, MusicPatterns: 0-4).
    pub style: u8,
    /// Sensitivity multiplier (0.5 - 2.0)
    pub sensitivity: f32,
    /// Smoothing factor (0.0 = instant, 0.9 = very smooth)
    pub smoothing: f32,
    /// Capture device name (exact or case-insensitive substring); None = auto-detect monitor source
    pub device: Option<String>,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            led_mode: cmd::LedMode::MusicBars.as_u8(),
            style: 0,
            sensitivity: 1.0,
            smoothing: 0.3,
            device: None,
        }
    }
}

/// Expand the analyzed [`NUM_BANDS`] normalized magnitudes (0.0-1.0) into the
/// keyboard's 16 audio-viz levels (0-[`audio_viz::MAX_LEVEL`]). Each analysis
/// band maps to two adjacent device bands.
fn bands_to_viz_levels(bands: &[f32; NUM_BANDS], sensitivity: f32) -> [u8; audio_viz::NUM_BANDS] {
    let mut levels = [0u8; audio_viz::NUM_BANDS];
    for (i, level) in levels.iter_mut().enumerate() {
        let band = bands[i * NUM_BANDS / audio_viz::NUM_BANDS];
        let v = (band * sensitivity).clamp(0.0, 1.0);
        *level = (v * audio_viz::MAX_LEVEL as f32).round() as u8;
    }
    levels
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

/// List available capture sources as human labels (description + `[monitor]` tag
/// + raw name). Returns an empty list if PulseAudio enumeration fails.
pub fn list_audio_devices() -> Vec<String> {
    pulse::list_sources()
        .unwrap_or_default()
        .iter()
        .map(pulse::SourceEntry::label)
        .collect()
}

/// Run audio reactive mode (blocking).
///
/// Starts audio capture in a background thread, switches the keyboard to its
/// native music-visualizer mode (MusicBars/MusicPatterns), and streams band
/// levels over `SET_AUDIO_VIZ` (0x0D) — the firmware renders the bars on-device.
/// No per-key SET_USERPIC streaming (no flash wear).
pub fn run_audio_reactive(
    keyboard: &KeyboardInterface,
    config: AudioConfig,
    running: Arc<AtomicBool>,
) -> Result<(), String> {
    println!("Starting audio capture...");

    // Start audio capture (creates stream and FFT processing thread)
    let audio_capture = AudioCapture::start(config.clone())?;

    println!("Audio input: {}", audio_capture.source_label);
    println!("Audio capture started, enabling music visualizer...");

    // Switch the keyboard into its native audio-viz mode (brightness/speed max).
    keyboard
        .set_music_viz_mode(config.led_mode, config.style, 4, 4, false)
        .map_err(|e| format!("Failed to set music visualizer mode: {e}"))?;
    thread::sleep(Duration::from_millis(200));

    // Stream band levels until stopped.
    run_viz_loop(keyboard, &audio_capture.state, &config, running);

    // Stop audio capture
    audio_capture.stop();

    println!("Audio reactive mode stopped");
    Ok(())
}

/// Visualizer loop — reads spectrum bands from [`AudioState`] and streams them to
/// the keyboard's on-device music visualizer via `SET_AUDIO_VIZ`.
pub fn run_viz_loop(
    keyboard: &KeyboardInterface,
    audio_state: &Arc<AudioState>,
    config: &AudioConfig,
    running: Arc<AtomicBool>,
) {
    let frame_duration = Duration::from_millis(audio_viz::UPDATE_INTERVAL_MS);
    let mut frame_count = 0u32;

    running.store(true, Ordering::SeqCst);

    while running.load(Ordering::SeqCst) && audio_state.is_running() {
        let frame_start = Instant::now();

        let bands = audio_state.get_bands();
        let levels = bands_to_viz_levels(&bands, config.sensitivity);
        let report = audio_viz::build_report(&levels);
        // Send the cmd payload (bytes after the leading command byte, through the
        // last band); the transport re-frames + checksums it. Use the no-delay
        // path — the default 100ms flow-control delay would cap us at ~10Hz.
        let _ = keyboard.send_raw_cmd_fast(cmd::SET_AUDIO_VIZ, &report[1..24]);

        // Debug output every ~5 seconds (only if RUST_LOG is set)
        frame_count += 1;
        if frame_count.is_multiple_of(audio_viz::UPDATE_RATE_HZ * 5)
            && std::env::var("RUST_LOG").is_ok()
        {
            let avg: f32 = bands.iter().sum::<f32>() / NUM_BANDS as f32;
            eprintln!("[viz] avg={avg:.2} bass={:.2} levels={levels:?}", bands[1]);
        }

        let elapsed = frame_start.elapsed();
        if elapsed < frame_duration {
            thread::sleep(frame_duration - elapsed);
        }
    }
}

/// Resolve the default capture source and confirm it opens.
pub fn test_audio_capture() -> Result<(), String> {
    let source = pulse::resolve_source(None)?;
    println!("Capture source: {}", source.label());
    println!("Format: {} Hz, mono f32", pulse::SAMPLE_RATE);
    let _simple = pulse::open_record(&source.name)?;
    println!("Stream opened OK.");
    Ok(())
}

/// Capture from a source and print a per-second peak level meter for 5 seconds.
pub fn test_audio_levels(requested_device: Option<&str>) -> Result<(), String> {
    use std::io::Write;

    let source = pulse::resolve_source(requested_device)?;
    println!("Using source: {}", source.label());
    println!("Format: {} Hz, mono f32", pulse::SAMPLE_RATE);

    let simple = pulse::open_record(&source.name)?;

    println!("\nListening for 5 seconds...");
    const READ_SAMPLES: usize = 882; // ~20 ms at 44.1 kHz
    let mut byte_buf = vec![0u8; READ_SAMPLES * 4];
    for i in 0..5 {
        let mut peak = 0.0f32;
        let mut reads = 0u32;
        let second_start = Instant::now();
        while second_start.elapsed() < Duration::from_secs(1) {
            simple
                .read(&mut byte_buf)
                .map_err(|e| format!("PulseAudio read failed: {e}"))?;
            reads += 1;
            for c in byte_buf.chunks_exact(4) {
                let s = f32::from_le_bytes([c[0], c[1], c[2], c[3]]).abs();
                peak = peak.max(s);
            }
        }
        let bars = (peak * 50.0).min(50.0) as usize;
        print!("  Second {}: {reads} reads, peak: {peak:.4} [", i + 1);
        for _ in 0..bars {
            print!("#");
        }
        for _ in bars..50 {
            print!(" ");
        }
        println!("]");
        std::io::stdout().flush().ok();
    }
    println!("\nDone.");
    Ok(())
}

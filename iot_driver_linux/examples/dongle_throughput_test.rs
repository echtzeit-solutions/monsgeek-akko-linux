//! Dongle transport throughput testing - measures minimum stable delays
//! and checks for input interface notifications.
//!
//! Usage:
//!   cargo run --example dongle_throughput_test -- delay-test
//!   cargo run --example dongle_throughput_test -- input-notify
//!   cargo run --example dongle_throughput_test -- full

use std::collections::HashMap;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use hidapi::{HidApi, HidDevice};

// Device constants
const VENDOR_ID: u16 = 0x3151;
const DONGLE_PID: u16 = 0x5038;
const USAGE_PAGE: u16 = 0xFFFF;
const USAGE_FEATURE: u16 = 0x02;
const USAGE_INPUT: u16 = 0x01;
const REPORT_SIZE: usize = 65;

// Commands with distinguishable responses
const CMD_GET_REV: u8 = 0x80;
const CMD_GET_PROFILE: u8 = 0x84;
const CMD_GET_LEDPARAM: u8 = 0x87;
const CMD_GET_USB_VERSION: u8 = 0x8F;
const CMD_FLUSH_NOP: u8 = 0xFC;

#[derive(Parser)]
#[command(name = "dongle_throughput_test")]
#[command(about = "Test dongle transport timing and notification mechanisms")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Test 1: Sweep delays to find minimum stable values
    DelayTest {
        /// Number of iterations per delay setting
        #[arg(short, long, default_value = "10")]
        iterations: u32,
        /// Minimum post-send delay to test (ms)
        #[arg(long, default_value = "0")]
        min_send_delay: u64,
        /// Maximum post-send delay to test (ms)
        #[arg(long, default_value = "200")]
        max_send_delay: u64,
        /// Delay step size (ms)
        #[arg(long, default_value = "20")]
        step: u64,
    },
    /// Test 2: Monitor input interface while sending commands
    InputNotify {
        /// Duration to run test (seconds)
        #[arg(short, long, default_value = "30")]
        duration: u64,
        /// Interval between commands (ms)
        #[arg(short, long, default_value = "500")]
        interval: u64,
    },
    /// Test 3: Poll as fast as possible to measure actual response latency
    PollTest {
        /// Number of queries per command
        #[arg(short, long, default_value = "50")]
        iterations: u32,
        /// Maximum poll attempts before timeout
        #[arg(long, default_value = "100")]
        max_polls: u32,
    },
    /// Debug: Show raw packet data during polling to check for sequence numbers
    PacketDebug {
        /// Number of commands to send
        #[arg(short, long, default_value = "3")]
        count: u32,
    },
    /// Test: Read without sending flush commands
    NoFlush,
    /// Test: Send multiple commands rapidly, see what responses come back
    Burst,
    /// Debug: Set LED param, read back to verify write succeeded
    SetReadTest {
        /// Number of set/read cycles
        #[arg(short, long, default_value = "5")]
        cycles: u32,
    },
    /// Debug: See exactly what response comes back after SET command
    SetResponseDebug,
    /// Combined test: run both tests sequentially
    Full {
        /// Iterations for delay test
        #[arg(short, long, default_value = "10")]
        iterations: u32,
    },
}

struct DongleDevices {
    feature: HidDevice,
    input: HidDevice,
}

fn open_dongle_devices() -> anyhow::Result<DongleDevices> {
    let api = HidApi::new()?;

    let mut feature_device = None;
    let mut input_device = None;

    for dev in api.device_list() {
        if dev.vendor_id() == VENDOR_ID
            && dev.product_id() == DONGLE_PID
            && dev.usage_page() == USAGE_PAGE
        {
            match dev.usage() {
                USAGE_FEATURE => {
                    feature_device = Some(dev.open_device(&api)?);
                    println!("Opened feature interface (usage=0x02)");
                }
                USAGE_INPUT => {
                    input_device = Some(dev.open_device(&api)?);
                    println!("Opened input interface (usage=0x01)");
                }
                _ => {}
            }
        }
    }

    let feature = feature_device
        .ok_or_else(|| anyhow::anyhow!("Feature interface not found (3151:5038 usage=0x02)"))?;
    let input = input_device
        .ok_or_else(|| anyhow::anyhow!("Input interface not found (3151:5038 usage=0x01)"))?;

    Ok(DongleDevices { feature, input })
}

fn calculate_checksum(data: &[u8]) -> u8 {
    let sum: u32 = data.iter().take(7).map(|&b| b as u32).sum();
    (255 - (sum & 0xFF)) as u8
}

fn build_command(cmd: u8) -> Vec<u8> {
    let mut buf = vec![0u8; REPORT_SIZE];
    buf[0] = 0; // Report ID
    buf[1] = cmd;
    buf[8] = calculate_checksum(&buf[1..]);
    buf
}

fn send_flush(device: &HidDevice) -> anyhow::Result<()> {
    let buf = build_command(CMD_FLUSH_NOP);
    device.send_feature_report(&buf)?;
    Ok(())
}

/// Result of a single command query attempt
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct QueryResult {
    success: bool,
    response_cmd: u8,
    latency_us: u64,
    retries_needed: u32,
}

fn query_with_timing(
    device: &HidDevice,
    cmd: u8,
    post_send_delay_ms: u64,
    post_flush_delay_ms: u64,
    max_retries: u32,
) -> QueryResult {
    let start = Instant::now();

    // Send command
    let buf = build_command(cmd);
    if device.send_feature_report(&buf).is_err() {
        return QueryResult {
            success: false,
            response_cmd: 0,
            latency_us: start.elapsed().as_micros() as u64,
            retries_needed: 0,
        };
    }

    if post_send_delay_ms > 0 {
        std::thread::sleep(Duration::from_millis(post_send_delay_ms));
    }

    // Flush and read pattern
    for attempt in 0..=max_retries {
        if send_flush(device).is_err() {
            continue;
        }

        if post_flush_delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(post_flush_delay_ms));
        }

        let mut resp = vec![0u8; REPORT_SIZE];
        resp[0] = 0;
        if device.get_feature_report(&mut resp).is_ok() {
            let resp_cmd = resp[1];
            if resp_cmd == cmd {
                return QueryResult {
                    success: true,
                    response_cmd: resp_cmd,
                    latency_us: start.elapsed().as_micros() as u64,
                    retries_needed: attempt,
                };
            }
        }
    }

    QueryResult {
        success: false,
        response_cmd: 0,
        latency_us: start.elapsed().as_micros() as u64,
        retries_needed: max_retries + 1,
    }
}

/// Statistics for a delay configuration
#[derive(Debug, Default)]
#[allow(dead_code)]
struct DelayStats {
    post_send_delay_ms: u64,
    post_flush_delay_ms: u64,
    total_queries: u32,
    successful: u32,
    avg_latency_us: u64,
    min_latency_us: u64,
    max_latency_us: u64,
    retries_histogram: [u32; 6], // 0-5 retries
}

fn run_delay_test(
    iterations: u32,
    min_delay: u64,
    max_delay: u64,
    step: u64,
) -> anyhow::Result<()> {
    println!("\n=== Dongle Delay Sweep Test ===\n");

    let devices = open_dongle_devices()?;
    let commands = [
        CMD_GET_REV,
        CMD_GET_PROFILE,
        CMD_GET_LEDPARAM,
        CMD_GET_USB_VERSION,
    ];

    println!(
        "\nTesting {} commands x {} iterations per delay setting",
        commands.len(),
        iterations
    );
    println!(
        "Delay range: {}ms - {}ms (step {}ms)\n",
        min_delay, max_delay, step
    );

    let mut results: Vec<DelayStats> = Vec::new();

    // Sweep post-send delay (post-flush stays fixed at baseline)
    let flush_delay = 100; // Current baseline

    for send_delay in (min_delay..=max_delay).step_by(step as usize) {
        let mut stats = DelayStats {
            post_send_delay_ms: send_delay,
            post_flush_delay_ms: flush_delay,
            min_latency_us: u64::MAX,
            ..Default::default()
        };

        let mut latencies = Vec::new();

        for _ in 0..iterations {
            for &cmd in &commands {
                let result = query_with_timing(&devices.feature, cmd, send_delay, flush_delay, 5);

                stats.total_queries += 1;
                if result.success {
                    stats.successful += 1;
                    latencies.push(result.latency_us);
                    stats.min_latency_us = stats.min_latency_us.min(result.latency_us);
                    stats.max_latency_us = stats.max_latency_us.max(result.latency_us);
                }

                let retry_idx = (result.retries_needed as usize).min(5);
                stats.retries_histogram[retry_idx] += 1;

                // Brief pause between queries to avoid overwhelming device
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        if !latencies.is_empty() {
            stats.avg_latency_us = latencies.iter().sum::<u64>() / latencies.len() as u64;
        }

        let success_rate = (stats.successful as f64 / stats.total_queries as f64) * 100.0;
        println!(
            "send={:3}ms flush={:3}ms | success: {:5.1}% ({:3}/{:3}) | avg_lat: {:6.2}ms | retries: {:?}",
            send_delay,
            flush_delay,
            success_rate,
            stats.successful,
            stats.total_queries,
            stats.avg_latency_us as f64 / 1000.0,
            stats.retries_histogram
        );

        results.push(stats);
    }

    // Summary: find minimum stable delay
    println!("\n=== Summary ===");
    let stable_threshold = 95.0;
    let mut found_stable = false;
    for stats in &results {
        let rate = (stats.successful as f64 / stats.total_queries as f64) * 100.0;
        if rate >= stable_threshold && !found_stable {
            println!(
                "Minimum stable post-send delay: {}ms ({:.1}% success)",
                stats.post_send_delay_ms, rate
            );
            found_stable = true;
        }
    }
    if !found_stable {
        println!(
            "No delay setting achieved {}% success rate",
            stable_threshold
        );
    }

    // Now sweep post-flush delay with best post-send delay
    let best_send = results
        .iter()
        .filter(|s| (s.successful as f64 / s.total_queries as f64) >= stable_threshold / 100.0)
        .map(|s| s.post_send_delay_ms)
        .next()
        .unwrap_or(150);

    println!(
        "\n=== Sweeping post-flush delay (send={}ms) ===\n",
        best_send
    );

    for flush_delay in (0..=150).step_by(25) {
        let mut stats = DelayStats {
            post_send_delay_ms: best_send,
            post_flush_delay_ms: flush_delay,
            min_latency_us: u64::MAX,
            ..Default::default()
        };

        let mut latencies = Vec::new();

        for _ in 0..iterations {
            for &cmd in &commands {
                let result = query_with_timing(&devices.feature, cmd, best_send, flush_delay, 5);

                stats.total_queries += 1;
                if result.success {
                    stats.successful += 1;
                    latencies.push(result.latency_us);
                }

                let retry_idx = (result.retries_needed as usize).min(5);
                stats.retries_histogram[retry_idx] += 1;

                std::thread::sleep(Duration::from_millis(5));
            }
        }

        if !latencies.is_empty() {
            stats.avg_latency_us = latencies.iter().sum::<u64>() / latencies.len() as u64;
        }

        let success_rate = (stats.successful as f64 / stats.total_queries as f64) * 100.0;
        println!(
            "send={:3}ms flush={:3}ms | success: {:5.1}% ({:3}/{:3}) | avg_lat: {:6.2}ms | retries: {:?}",
            best_send,
            flush_delay,
            success_rate,
            stats.successful,
            stats.total_queries,
            stats.avg_latency_us as f64 / 1000.0,
            stats.retries_histogram
        );
    }

    Ok(())
}

/// Event observation from input interface
#[derive(Debug)]
struct InputEvent {
    timestamp_secs: f32,
    data: Vec<u8>,
    context: String,
}

fn run_input_notify_test(duration_secs: u64, interval_ms: u64) -> anyhow::Result<()> {
    println!("\n=== Input Interface Notification Test ===\n");

    let devices = open_dongle_devices()?;

    // Set input device to non-blocking
    devices.input.set_blocking_mode(false)?;

    let mut events: Vec<InputEvent> = Vec::new();
    let mut commands_sent: u32 = 0;
    let mut commands_successful: u32 = 0;

    let start = Instant::now();
    let test_duration = Duration::from_secs(duration_secs);
    let command_interval = Duration::from_millis(interval_ms);
    let commands = [CMD_GET_REV, CMD_GET_PROFILE, CMD_GET_LEDPARAM];
    let mut cmd_idx = 0;
    let mut next_command_time = Instant::now();

    println!(
        "Monitoring input interface for {} seconds...",
        duration_secs
    );
    println!("Sending commands every {}ms\n", interval_ms);

    while start.elapsed() < test_duration {
        // Check for input events (non-blocking)
        let mut buf = vec![0u8; 64];
        match devices.input.read_timeout(&mut buf, 10) {
            Ok(len) if len > 0 => {
                let elapsed = start.elapsed().as_secs_f32();
                events.push(InputEvent {
                    timestamp_secs: elapsed,
                    data: buf[..len].to_vec(),
                    context: "polled".to_string(),
                });
                println!(
                    "[{:6.2}s] INPUT EVENT: {} bytes: {:02X?}",
                    elapsed,
                    len,
                    &buf[..len.min(16)]
                );
            }
            _ => {}
        }

        // Send periodic commands
        if Instant::now() >= next_command_time {
            let cmd = commands[cmd_idx % commands.len()];
            cmd_idx += 1;

            // Before command - quick poll
            let mut buf = vec![0u8; 64];
            if let Ok(len) = devices.input.read_timeout(&mut buf, 1) {
                if len > 0 {
                    events.push(InputEvent {
                        timestamp_secs: start.elapsed().as_secs_f32(),
                        data: buf[..len].to_vec(),
                        context: "before_cmd".to_string(),
                    });
                }
            }

            // Send command
            let cmd_buf = build_command(cmd);
            if devices.feature.send_feature_report(&cmd_buf).is_ok() {
                commands_sent += 1;

                // Check input immediately after send
                let mut buf = vec![0u8; 64];
                for _ in 0..5 {
                    if let Ok(len) = devices.input.read_timeout(&mut buf, 5) {
                        if len > 0 {
                            let elapsed = start.elapsed().as_secs_f32();
                            events.push(InputEvent {
                                timestamp_secs: elapsed,
                                data: buf[..len].to_vec(),
                                context: format!("after_send_0x{:02X}", cmd),
                            });
                            println!(
                                "[{:6.2}s] Event after cmd 0x{:02X}: {:02X?}",
                                elapsed,
                                cmd,
                                &buf[..len.min(16)]
                            );
                        }
                    }
                }

                // Complete the query
                std::thread::sleep(Duration::from_millis(50));
                let _ = send_flush(&devices.feature);
                std::thread::sleep(Duration::from_millis(50));

                let mut resp = vec![0u8; REPORT_SIZE];
                resp[0] = 0;
                if devices.feature.get_feature_report(&mut resp).is_ok() && resp[1] == cmd {
                    commands_successful += 1;
                }

                // Check input after flush/read
                let mut buf = vec![0u8; 64];
                if let Ok(len) = devices.input.read_timeout(&mut buf, 5) {
                    if len > 0 {
                        events.push(InputEvent {
                            timestamp_secs: start.elapsed().as_secs_f32(),
                            data: buf[..len].to_vec(),
                            context: format!("after_read_0x{:02X}", cmd),
                        });
                    }
                }
            }

            next_command_time = Instant::now() + command_interval;
        }

        std::thread::sleep(Duration::from_millis(1));
    }

    // Summary
    println!("\n=== Input Notification Test Summary ===");
    println!("Duration: {:.1}s", start.elapsed().as_secs_f32());
    println!("Commands sent: {}", commands_sent);
    if commands_sent > 0 {
        println!(
            "Commands successful: {} ({:.1}%)",
            commands_successful,
            (commands_successful as f64 / commands_sent as f64) * 100.0
        );
    }
    println!("Input events observed: {}", events.len());

    if !events.is_empty() {
        println!("\nEvent details:");
        for (i, event) in events.iter().enumerate() {
            println!(
                "  {}. [{:6.2}s] [{}] {:02X?}",
                i + 1,
                event.timestamp_secs,
                event.context,
                &event.data[..event.data.len().min(16)]
            );
        }

        // Categorize events
        let mut by_context: HashMap<String, u32> = HashMap::new();
        for event in &events {
            *by_context.entry(event.context.clone()).or_insert(0) += 1;
        }
        println!("\nEvents by context:");
        for (ctx, count) in by_context {
            println!("  {}: {}", ctx, count);
        }
    } else {
        println!(
            "\nNo input events observed - input interface does NOT receive command notifications"
        );
    }

    Ok(())
}

/// Result of a polling query
#[derive(Debug)]
struct PollResult {
    success: bool,
    latency_us: u64,
    polls_needed: u32,
    time_to_first_flush_us: u64,
    time_per_poll_us: Vec<u64>,
}

fn query_with_polling(device: &HidDevice, cmd: u8, max_polls: u32) -> PollResult {
    let start = Instant::now();

    // Send command
    let buf = build_command(cmd);
    if device.send_feature_report(&buf).is_err() {
        return PollResult {
            success: false,
            latency_us: start.elapsed().as_micros() as u64,
            polls_needed: 0,
            time_to_first_flush_us: 0,
            time_per_poll_us: vec![],
        };
    }

    let after_send = start.elapsed();
    let mut poll_times = Vec::new();

    // Poll as fast as possible
    for poll in 0..max_polls {
        let poll_start = Instant::now();

        // Send flush
        if send_flush(device).is_err() {
            poll_times.push(poll_start.elapsed().as_micros() as u64);
            continue;
        }

        // Read immediately
        let mut resp = vec![0u8; REPORT_SIZE];
        resp[0] = 0;
        if device.get_feature_report(&mut resp).is_ok() {
            let resp_cmd = resp[1];
            poll_times.push(poll_start.elapsed().as_micros() as u64);

            if resp_cmd == cmd {
                return PollResult {
                    success: true,
                    latency_us: start.elapsed().as_micros() as u64,
                    polls_needed: poll + 1,
                    time_to_first_flush_us: after_send.as_micros() as u64,
                    time_per_poll_us: poll_times,
                };
            }
        } else {
            poll_times.push(poll_start.elapsed().as_micros() as u64);
        }
    }

    PollResult {
        success: false,
        latency_us: start.elapsed().as_micros() as u64,
        polls_needed: max_polls,
        time_to_first_flush_us: after_send.as_micros() as u64,
        time_per_poll_us: poll_times,
    }
}

/// Debug version that prints all poll responses
fn query_with_polling_debug(device: &HidDevice, cmd: u8, max_polls: u32) {
    // Send command
    let buf = build_command(cmd);
    println!("TX cmd 0x{:02X}: {:02X?}", cmd, &buf[..16]);

    if device.send_feature_report(&buf).is_err() {
        println!("  send failed!");
        return;
    }

    // Poll and show each response
    for poll in 0..max_polls {
        // Send flush
        let flush_buf = build_command(CMD_FLUSH_NOP);
        if device.send_feature_report(&flush_buf).is_err() {
            println!("  poll {}: flush failed", poll);
            continue;
        }

        // Read
        let mut resp = vec![0u8; REPORT_SIZE];
        resp[0] = 0;
        if device.get_feature_report(&mut resp).is_ok() {
            let resp_cmd = resp[1];
            println!(
                "  poll {:2}: cmd=0x{:02X} data={:02X?}",
                poll,
                resp_cmd,
                &resp[1..17]
            );

            if resp_cmd == cmd {
                println!("  -> MATCH found at poll {}", poll);
                return;
            }
        } else {
            println!("  poll {}: read failed", poll);
        }
    }
    println!("  -> TIMEOUT after {} polls", max_polls);
}

fn run_poll_test(iterations: u32, max_polls: u32) -> anyhow::Result<()> {
    println!("\n=== Dongle Polling Latency Test ===\n");

    let api = HidApi::new()?;
    let mut feature_device = None;

    for dev in api.device_list() {
        if dev.vendor_id() == VENDOR_ID
            && dev.product_id() == DONGLE_PID
            && dev.usage_page() == USAGE_PAGE
            && dev.usage() == USAGE_FEATURE
        {
            feature_device = Some(dev.open_device(&api)?);
            println!("Opened feature interface (usage=0x02)");
            break;
        }
    }

    let device = feature_device.ok_or_else(|| anyhow::anyhow!("Dongle not found"))?;

    let commands = [
        (CMD_GET_REV, "GET_REV"),
        (CMD_GET_PROFILE, "GET_PROFILE"),
        (CMD_GET_LEDPARAM, "GET_LEDPARAM"),
        (CMD_GET_USB_VERSION, "GET_USB_VERSION"),
    ];

    println!(
        "\nPolling with zero delays (max {} polls per query)",
        max_polls
    );
    println!("Running {} iterations per command\n", iterations);

    for (cmd, name) in &commands {
        let mut latencies = Vec::new();
        let mut polls_needed = Vec::new();
        let mut successes = 0u32;
        let mut poll_time_samples: Vec<u64> = Vec::new();

        for _ in 0..iterations {
            let result = query_with_polling(&device, *cmd, max_polls);

            if result.success {
                successes += 1;
                latencies.push(result.latency_us);
                polls_needed.push(result.polls_needed);
                poll_time_samples.extend(result.time_per_poll_us.iter());
            }

            // Small gap between queries
            std::thread::sleep(Duration::from_millis(2));
        }

        if !latencies.is_empty() {
            latencies.sort();
            polls_needed.sort();

            let avg_latency = latencies.iter().sum::<u64>() / latencies.len() as u64;
            let min_latency = latencies[0];
            let max_latency = latencies[latencies.len() - 1];
            let p50_latency = latencies[latencies.len() / 2];
            let p95_idx = (latencies.len() as f64 * 0.95) as usize;
            let p95_latency = latencies[p95_idx.min(latencies.len() - 1)];

            let avg_polls = polls_needed.iter().sum::<u32>() as f64 / polls_needed.len() as f64;
            let min_polls = polls_needed[0];
            let max_polls_seen = polls_needed[polls_needed.len() - 1];

            // Calculate average time per flush+read cycle
            let avg_poll_time = if !poll_time_samples.is_empty() {
                poll_time_samples.iter().sum::<u64>() / poll_time_samples.len() as u64
            } else {
                0
            };

            println!("0x{:02X} {:<14} | success: {:3}/{:3} | latency: avg={:6.2}ms min={:6.2}ms max={:6.2}ms p50={:6.2}ms p95={:6.2}ms",
                cmd, name, successes, iterations,
                avg_latency as f64 / 1000.0,
                min_latency as f64 / 1000.0,
                max_latency as f64 / 1000.0,
                p50_latency as f64 / 1000.0,
                p95_latency as f64 / 1000.0);
            println!(
                "                  | polls: avg={:.1} min={} max={} | per_poll: {:.2}ms",
                avg_polls,
                min_polls,
                max_polls_seen,
                avg_poll_time as f64 / 1000.0
            );
        } else {
            println!("0x{:02X} {:<14} | ALL FAILED", cmd, name);
        }
    }

    // Summary statistics
    println!("\n=== Summary ===");
    println!("Each poll cycle = send_flush + get_feature_report (no delays)");
    println!("Response typically ready after 1-2 poll cycles");

    Ok(())
}

/// Test reading without flush - just repeated get_feature_report
fn run_no_flush_test() -> anyhow::Result<()> {
    println!("\n=== Test: Read Without Flush ===\n");

    let api = HidApi::new()?;
    let mut feature_device = None;

    for dev in api.device_list() {
        if dev.vendor_id() == VENDOR_ID
            && dev.product_id() == DONGLE_PID
            && dev.usage_page() == USAGE_PAGE
            && dev.usage() == USAGE_FEATURE
        {
            feature_device = Some(dev.open_device(&api)?);
            break;
        }
    }

    let device = feature_device.ok_or_else(|| anyhow::anyhow!("Dongle not found"))?;

    // Test 1: Send command, then just read repeatedly (no flush)
    println!("Test 1: Send GET_PROFILE, read without flush");
    let cmd = CMD_GET_PROFILE;
    let buf = build_command(cmd);
    println!("TX: {:02X?}", &buf[..16]);
    device.send_feature_report(&buf)?;

    for i in 0..15 {
        let mut resp = vec![0u8; REPORT_SIZE];
        resp[0] = 0;
        if device.get_feature_report(&mut resp).is_ok() {
            println!(
                "  read {:2}: cmd=0x{:02X} data={:02X?}",
                i,
                resp[1],
                &resp[1..12]
            );
            if resp[1] == cmd {
                println!("  -> MATCH at read {}", i);
                break;
            }
        }
    }

    std::thread::sleep(Duration::from_millis(100));

    // Test 2: Send command, wait, then read once (no flush)
    println!("\nTest 2: Send GET_LEDPARAM, wait 50ms, read without flush");
    let cmd = CMD_GET_LEDPARAM;
    let buf = build_command(cmd);
    println!("TX: {:02X?}", &buf[..16]);
    device.send_feature_report(&buf)?;
    std::thread::sleep(Duration::from_millis(50));

    for i in 0..5 {
        let mut resp = vec![0u8; REPORT_SIZE];
        resp[0] = 0;
        if device.get_feature_report(&mut resp).is_ok() {
            println!(
                "  read {:2}: cmd=0x{:02X} data={:02X?}",
                i,
                resp[1],
                &resp[1..12]
            );
            if resp[1] == cmd {
                println!("  -> MATCH at read {}", i);
                break;
            }
        }
    }

    std::thread::sleep(Duration::from_millis(100));

    // Test 3: Compare with flush approach
    println!("\nTest 3: Send GET_USB_VERSION with single flush, then read");
    let cmd = CMD_GET_USB_VERSION;
    let buf = build_command(cmd);
    println!("TX: {:02X?}", &buf[..16]);
    device.send_feature_report(&buf)?;
    std::thread::sleep(Duration::from_millis(20));

    // Single flush
    send_flush(&device)?;
    std::thread::sleep(Duration::from_millis(20));

    for i in 0..5 {
        let mut resp = vec![0u8; REPORT_SIZE];
        resp[0] = 0;
        if device.get_feature_report(&mut resp).is_ok() {
            println!(
                "  read {:2}: cmd=0x{:02X} data={:02X?}",
                i,
                resp[1],
                &resp[1..12]
            );
            if resp[1] == cmd {
                println!("  -> MATCH at read {}", i);
                break;
            }
        }
    }

    Ok(())
}

/// Test sending multiple commands rapidly
fn run_burst_test() -> anyhow::Result<()> {
    println!("\n=== Test: Rapid Command Burst ===\n");

    let api = HidApi::new()?;
    let mut feature_device = None;

    for dev in api.device_list() {
        if dev.vendor_id() == VENDOR_ID
            && dev.product_id() == DONGLE_PID
            && dev.usage_page() == USAGE_PAGE
            && dev.usage() == USAGE_FEATURE
        {
            feature_device = Some(dev.open_device(&api)?);
            break;
        }
    }

    let device = feature_device.ok_or_else(|| anyhow::anyhow!("Dongle not found"))?;

    // Send 4 different commands as fast as possible
    let commands = [
        (CMD_GET_REV, "GET_REV"),
        (CMD_GET_PROFILE, "GET_PROFILE"),
        (CMD_GET_LEDPARAM, "GET_LEDPARAM"),
        (CMD_GET_USB_VERSION, "GET_USB_VERSION"),
    ];

    println!("Sending 4 commands rapidly without waiting...\n");

    for (cmd, name) in &commands {
        let buf = build_command(*cmd);
        device.send_feature_report(&buf)?;
        println!("TX 0x{:02X} {}", cmd, name);
    }

    println!("\nNow polling for responses...\n");

    let mut found = [false; 4];
    let mut responses: Vec<(u8, Vec<u8>)> = Vec::new();

    // Poll and collect all responses
    for poll in 0..30 {
        send_flush(&device)?;

        let mut resp = vec![0u8; REPORT_SIZE];
        resp[0] = 0;
        if device.get_feature_report(&mut resp).is_ok() {
            let resp_cmd = resp[1];
            // Show all responses including zeros
            println!(
                "  poll {:2}: cmd=0x{:02X} data={:02X?}",
                poll,
                resp_cmd,
                &resp[1..12]
            );

            if resp_cmd != 0 && resp_cmd != CMD_FLUSH_NOP {
                responses.push((resp_cmd, resp[1..12].to_vec()));

                // Mark as found
                for (i, (cmd, _)) in commands.iter().enumerate() {
                    if resp_cmd == *cmd {
                        found[i] = true;
                    }
                }

                // Check if we got all
                if found.iter().all(|&f| f) {
                    println!("\n  -> All 4 responses received!");
                    break;
                }
            }
        }
    }

    println!("\nResults:");
    for (i, (cmd, name)) in commands.iter().enumerate() {
        println!(
            "  0x{:02X} {}: {}",
            cmd,
            name,
            if found[i] { "FOUND" } else { "MISSING" }
        );
    }

    if !responses.is_empty() {
        println!("\nResponse order received:");
        for (i, (cmd, _)) in responses.iter().enumerate() {
            let name = commands
                .iter()
                .find(|(c, _)| *c == *cmd)
                .map(|(_, n)| *n)
                .unwrap_or("?");
            println!("  {}: 0x{:02X} {}", i + 1, cmd, name);
        }
    }

    Ok(())
}

const CMD_SET_LEDPARAM: u8 = 0x07;

/// Debug test: Set LED param and read back to verify
fn run_set_read_test(cycles: u32) -> anyhow::Result<()> {
    println!("\n=== Set/Read Verification Test ===\n");

    let api = HidApi::new()?;
    let mut feature_device = None;

    for dev in api.device_list() {
        if dev.vendor_id() == VENDOR_ID
            && dev.product_id() == DONGLE_PID
            && dev.usage_page() == USAGE_PAGE
            && dev.usage() == USAGE_FEATURE
        {
            feature_device = Some(dev.open_device(&api)?);
            break;
        }
    }

    let device = feature_device.ok_or_else(|| anyhow::anyhow!("Dongle not found"))?;
    println!("Opened dongle feature interface\n");

    // Helper to query with polling
    let query = |dev: &HidDevice, cmd: u8| -> Option<Vec<u8>> {
        let buf = build_command(cmd);
        if dev.send_feature_report(&buf).is_err() {
            return None;
        }

        for _ in 0..50 {
            let flush_buf = build_command(CMD_FLUSH_NOP);
            if dev.send_feature_report(&flush_buf).is_err() {
                continue;
            }

            let mut resp = vec![0u8; REPORT_SIZE];
            resp[0] = 0;
            if dev.get_feature_report(&mut resp).is_ok() && resp[1] == cmd {
                return Some(resp);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        None
    };

    // Helper to send SET command and wait
    let send_set = |dev: &HidDevice, cmd: u8, data: &[u8]| -> bool {
        let mut buf = vec![0u8; REPORT_SIZE];
        buf[0] = 0;
        buf[1] = cmd;
        let len = std::cmp::min(data.len(), REPORT_SIZE - 2);
        buf[2..2 + len].copy_from_slice(&data[..len]);
        // Calculate checksum (Bit7)
        let sum: u32 = buf[1..8].iter().map(|&b| b as u32).sum();
        buf[8] = (255 - (sum & 0xFF)) as u8;

        if dev.send_feature_report(&buf).is_err() {
            return false;
        }

        // Wait a bit for command to be processed
        std::thread::sleep(Duration::from_millis(20));

        // Send flush to push any pending response
        let flush_buf = build_command(CMD_FLUSH_NOP);
        let _ = dev.send_feature_report(&flush_buf);
        std::thread::sleep(Duration::from_millis(10));

        true
    };

    // Read initial LED params
    println!("Reading initial LED params...");
    let initial = query(&device, CMD_GET_LEDPARAM);
    if initial.is_none() {
        println!("ERROR: Failed to read initial LED params");
        return Ok(());
    }
    let initial = initial.unwrap();
    println!(
        "Initial: mode={} brightness={} speed={} rgb=({},{},{}) dir={}",
        initial[2], initial[3], initial[4], initial[5], initial[6], initial[7], initial[8]
    );

    let original_brightness = initial[3];
    let mut success_count = 0u32;
    let mut fail_count = 0u32;

    for cycle in 0..cycles {
        // Alternate brightness between two values
        let new_brightness = if cycle % 2 == 0 {
            (original_brightness.wrapping_add(10)) % 101
        } else {
            original_brightness
        };

        // Build SET_LEDPARAM data: [mode, brightness, speed, r, g, b, direction]
        let set_data = [
            initial[2],     // mode
            new_brightness, // brightness (changed)
            initial[4],     // speed
            initial[5],     // r
            initial[6],     // g
            initial[7],     // b
            initial[8],     // direction
        ];

        println!(
            "\nCycle {}: Setting brightness to {}...",
            cycle + 1,
            new_brightness
        );

        let set_start = Instant::now();
        if !send_set(&device, CMD_SET_LEDPARAM, &set_data) {
            println!("  SET failed!");
            fail_count += 1;
            continue;
        }
        let set_time = set_start.elapsed();

        // Read back
        let read_start = Instant::now();
        let readback = query(&device, CMD_GET_LEDPARAM);
        let read_time = read_start.elapsed();

        if let Some(rb) = readback {
            let rb_brightness = rb[3];
            if rb_brightness == new_brightness {
                println!(
                    "  OK: Read back brightness={} (set: {:.1}ms, read: {:.1}ms)",
                    rb_brightness,
                    set_time.as_secs_f64() * 1000.0,
                    read_time.as_secs_f64() * 1000.0
                );
                success_count += 1;
            } else {
                println!(
                    "  MISMATCH: Expected {}, got {} (set: {:.1}ms, read: {:.1}ms)",
                    new_brightness,
                    rb_brightness,
                    set_time.as_secs_f64() * 1000.0,
                    read_time.as_secs_f64() * 1000.0
                );
                println!("  Full response: {:02X?}", &rb[1..10]);
                fail_count += 1;
            }
        } else {
            println!("  READ failed!");
            fail_count += 1;
        }
    }

    // Restore original brightness
    println!("\nRestoring original brightness {}...", original_brightness);
    let restore_data = [
        initial[2],
        original_brightness,
        initial[4],
        initial[5],
        initial[6],
        initial[7],
        initial[8],
    ];
    send_set(&device, CMD_SET_LEDPARAM, &restore_data);

    println!("\n=== Results ===");
    println!("Success: {}/{}", success_count, cycles);
    println!("Failed:  {}/{}", fail_count, cycles);
    if fail_count > 0 {
        println!("\nPossible issues:");
        println!("  - SET command may need longer wait before read");
        println!("  - Response caching may be returning stale data");
        println!("  - Keyboard may not ACK SET commands immediately");
    }

    Ok(())
}

/// Debug what exactly the keyboard responds with after a SET command
fn run_set_response_debug() -> anyhow::Result<()> {
    println!("\n=== SET Response Debug ===\n");
    println!("Shows exactly what response comes back after SET_LEDPARAM (0x07)\n");

    let api = HidApi::new()?;
    let mut feature_device = None;

    for dev in api.device_list() {
        if dev.vendor_id() == VENDOR_ID
            && dev.product_id() == DONGLE_PID
            && dev.usage_page() == USAGE_PAGE
            && dev.usage() == USAGE_FEATURE
        {
            feature_device = Some(dev.open_device(&api)?);
            break;
        }
    }

    let device = feature_device.ok_or_else(|| anyhow::anyhow!("Dongle not found"))?;
    println!("Opened dongle feature interface\n");

    // First, read current LED params
    println!("1. Reading current LED params (0x87)...");
    let buf = build_command(CMD_GET_LEDPARAM);
    device.send_feature_report(&buf)?;

    // Poll for response
    let mut current_params = None;
    for _ in 0..50 {
        let flush_buf = build_command(CMD_FLUSH_NOP);
        device.send_feature_report(&flush_buf)?;

        let mut resp = vec![0u8; REPORT_SIZE];
        resp[0] = 0;
        if device.get_feature_report(&mut resp).is_ok() && resp[1] == CMD_GET_LEDPARAM {
            println!("   GET response: {:02X?}", &resp[1..16]);
            current_params = Some(resp);
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    let params = current_params.ok_or_else(|| anyhow::anyhow!("Failed to read LED params"))?;
    println!(
        "   Mode={} Brightness={} Speed={} RGB=({},{},{}) Dir={}",
        params[2], params[3], params[4], params[5], params[6], params[7], params[8]
    );

    // Now send SET command with slightly different brightness
    let new_brightness = (params[3].wrapping_add(5)) % 101;
    println!(
        "\n2. Sending SET_LEDPARAM (0x07) with brightness {}...",
        new_brightness
    );

    let mut set_buf = vec![0u8; REPORT_SIZE];
    set_buf[0] = 0;
    set_buf[1] = CMD_SET_LEDPARAM;
    set_buf[2] = params[2]; // mode
    set_buf[3] = new_brightness; // brightness
    set_buf[4] = params[4]; // speed
    set_buf[5] = params[5]; // r
    set_buf[6] = params[6]; // g
    set_buf[7] = params[7]; // b
    set_buf[8] = params[8]; // direction
                            // Checksum
    let sum: u32 = set_buf[1..8].iter().map(|&b| b as u32).sum();
    set_buf[8] = (255 - (sum & 0xFF)) as u8;

    println!("   Sent: {:02X?}", &set_buf[1..16]);
    device.send_feature_report(&set_buf)?;

    // Now poll and show ALL responses we get
    println!("\n3. Polling for response (showing all non-zero responses)...");
    let poll_start = Instant::now();
    let mut responses_seen = Vec::new();

    for poll in 0..100 {
        let flush_buf = build_command(CMD_FLUSH_NOP);
        device.send_feature_report(&flush_buf)?;

        let mut resp = vec![0u8; REPORT_SIZE];
        resp[0] = 0;
        if device.get_feature_report(&mut resp).is_ok() {
            let cmd = resp[1];
            // Show any non-zero, non-flush response
            if cmd != 0 && cmd != CMD_FLUSH_NOP && resp[2..16].iter().any(|&b| b != 0) {
                let elapsed = poll_start.elapsed();
                println!(
                    "   Poll {}: cmd=0x{:02X} @ {:.1}ms: {:02X?}",
                    poll,
                    cmd,
                    elapsed.as_secs_f64() * 1000.0,
                    &resp[1..16]
                );
                responses_seen.push((cmd, resp.clone()));

                // After finding response, check a few more times
                if responses_seen.len() >= 3 {
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    if responses_seen.is_empty() {
        println!("   No response received within timeout!");
    }

    // Analyze what we saw
    println!("\n=== Analysis ===");
    if responses_seen
        .iter()
        .any(|(cmd, _)| *cmd == CMD_SET_LEDPARAM)
    {
        println!(
            "SET command (0x{:02X}) echoes back the same command byte",
            CMD_SET_LEDPARAM
        );
    } else if responses_seen
        .iter()
        .any(|(cmd, _)| *cmd == CMD_GET_LEDPARAM)
    {
        println!(
            "SET command causes GET response (0x{:02X}) instead of SET echo",
            CMD_GET_LEDPARAM
        );
        println!("=> Transport should wait for 0x87, not 0x07!");
    } else if !responses_seen.is_empty() {
        println!(
            "SET command responds with different command: 0x{:02X}",
            responses_seen[0].0
        );
    } else {
        println!("SET command does NOT produce any response!");
        println!("=> send_command should NOT wait for echo");
    }

    // Restore original value
    println!("\n4. Restoring original brightness {}...", params[3]);
    set_buf[3] = params[3];
    let sum: u32 = set_buf[1..8].iter().map(|&b| b as u32).sum();
    set_buf[8] = (255 - (sum & 0xFF)) as u8;
    device.send_feature_report(&set_buf)?;

    Ok(())
}

fn run_packet_debug(count: u32) -> anyhow::Result<()> {
    println!("\n=== Packet Debug - Raw Response Data ===\n");

    let api = HidApi::new()?;
    let mut feature_device = None;

    for dev in api.device_list() {
        if dev.vendor_id() == VENDOR_ID
            && dev.product_id() == DONGLE_PID
            && dev.usage_page() == USAGE_PAGE
            && dev.usage() == USAGE_FEATURE
        {
            feature_device = Some(dev.open_device(&api)?);
            println!("Opened feature interface\n");
            break;
        }
    }

    let device = feature_device.ok_or_else(|| anyhow::anyhow!("Dongle not found"))?;

    let commands = [
        (CMD_GET_REV, "GET_REV"),
        (CMD_GET_PROFILE, "GET_PROFILE"),
        (CMD_GET_LEDPARAM, "GET_LEDPARAM"),
        (CMD_GET_USB_VERSION, "GET_USB_VERSION"),
    ];

    for round in 0..count {
        println!("--- Round {} ---", round + 1);
        for (cmd, name) in &commands {
            println!("\n[{}]", name);
            query_with_polling_debug(&device, *cmd, 15);
            std::thread::sleep(Duration::from_millis(50));
        }
        println!();
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::DelayTest {
            iterations,
            min_send_delay,
            max_send_delay,
            step,
        } => {
            run_delay_test(iterations, min_send_delay, max_send_delay, step)?;
        }
        Commands::InputNotify { duration, interval } => {
            run_input_notify_test(duration, interval)?;
        }
        Commands::PollTest {
            iterations,
            max_polls,
        } => {
            run_poll_test(iterations, max_polls)?;
        }
        Commands::PacketDebug { count } => {
            run_packet_debug(count)?;
        }
        Commands::NoFlush => {
            run_no_flush_test()?;
        }
        Commands::Burst => {
            run_burst_test()?;
        }
        Commands::SetReadTest { cycles } => {
            run_set_read_test(cycles)?;
        }
        Commands::SetResponseDebug => {
            run_set_response_debug()?;
        }
        Commands::Full { iterations } => {
            run_delay_test(iterations, 0, 200, 20)?;
            println!("\n{}\n", "=".repeat(60));
            run_input_notify_test(30, 500)?;
        }
    }

    Ok(())
}

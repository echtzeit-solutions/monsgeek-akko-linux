//! Integration tests for keyboard device queries.
//!
//! These tests require a real keyboard to be connected.
//! Run with: cargo test -p monsgeek-keyboard --test device_queries -- --ignored --nocapture

use std::sync::Arc;
use std::time::Duration;

use monsgeek_keyboard::{KeyboardInterface, PollingRate, Precision};
use monsgeek_transport::{ChecksumType, FlowControlTransport, HidDiscovery, Transport};

/// Open the preferred keyboard and create a KeyboardInterface.
///
/// Mirrors the TUI's `connect()` flow:
/// HidDiscovery → open_preferred → FlowControlTransport → KeyboardInterface
fn open_keyboard() -> (Arc<dyn Transport>, KeyboardInterface) {
    let discovery = HidDiscovery::new();
    let transport = discovery
        .open_preferred()
        .expect("No keyboard found — plug in a supported device");

    let info = transport.device_info();
    let (key_count, has_magnetism) = match (info.vid, info.pid) {
        (0x3151, 0x5030 | 0x5038 | 0x5039) => (98, true),
        _ => (98, true),
    };

    let flow = Arc::new(FlowControlTransport::new(Arc::clone(&transport)));
    let kb = KeyboardInterface::new(flow, key_count, has_magnetism);
    (transport, kb)
}

/// Reproduces the TUI first-tab load: all 9 queries the "Device Info" tab
/// fires as separate `tokio::spawn` tasks on connect.
///
/// Uses `tokio::spawn` per query (not `tokio::join!`) to match the TUI's
/// actual concurrency pattern. All must resolve within 5 seconds.
#[tokio::test(flavor = "multi_thread")]
#[ignore] // requires hardware
async fn tui_first_tab_queries_resolve() {
    let (_raw, kb) = open_keyboard();
    let kb = Arc::new(kb);

    // Same queries as tui.rs load_device_info(), each spawned as a
    // separate task — exactly how the TUI does it
    let h_device_id = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_device_id() })
    };
    let h_version = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_version() })
    };
    let h_profile = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_profile() })
    };
    let h_debounce = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_debounce() })
    };
    let h_poll_rate = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_polling_rate() })
    };
    let h_led = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_led_params() })
    };
    let h_side_led = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_side_led_params() })
    };
    let h_kb_opts = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_kb_options() })
    };
    let h_precision = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_precision() })
    };
    let h_sleep = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_sleep_time() })
    };

    let results = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::try_join!(
            h_device_id,
            h_version,
            h_profile,
            h_debounce,
            h_poll_rate,
            h_led,
            h_side_led,
            h_kb_opts,
            h_precision,
            h_sleep,
        )
    })
    .await
    .expect("TUI first-tab queries did not complete within 5 seconds")
    .expect("A spawned task panicked");

    let (
        device_id,
        version,
        profile,
        debounce,
        polling_rate,
        led_params,
        side_led,
        kb_options,
        precision,
        sleep_time,
    ) = results;

    // All queries must succeed
    let device_id = device_id.expect("get_device_id failed");
    let version = version.expect("get_version failed");
    let profile = profile.expect("get_profile failed");
    let debounce = debounce.expect("get_debounce failed");
    let polling_rate = polling_rate.expect("get_polling_rate failed");
    let led_params = led_params.expect("get_led_params failed");
    let _side_led = side_led.expect("get_side_led_params failed");
    let kb_options = kb_options.expect("get_kb_options failed");
    let precision = precision.expect("get_precision failed");
    let sleep_time = sleep_time.expect("get_sleep_time failed");

    // Sanity checks — values should be in plausible ranges
    assert!(device_id > 0, "device_id should be non-zero");
    assert!(version.raw > 0, "firmware version raw should be non-zero");
    assert!(profile <= 3, "profile should be 0-3, got {profile}");
    assert!(debounce <= 50, "debounce should be 0-50ms, got {debounce}");
    assert!(
        matches!(
            polling_rate,
            PollingRate::Hz125
                | PollingRate::Hz250
                | PollingRate::Hz500
                | PollingRate::Hz1000
                | PollingRate::Hz2000
                | PollingRate::Hz4000
                | PollingRate::Hz8000
        ),
        "polling_rate should be a known rate"
    );
    assert!(
        led_params.brightness <= 4,
        "brightness should be 0-4, got {}",
        led_params.brightness
    );
    assert!(
        matches!(
            precision,
            Precision::Coarse | Precision::Medium | Precision::Fine
        ),
        "precision should be a known variant"
    );

    eprintln!("=== TUI first-tab query results ===");
    eprintln!("  Device ID:    {} (0x{:08x})", device_id, device_id);
    eprintln!("  Firmware:     {}", version.format());
    eprintln!("  Profile:      {}", profile);
    eprintln!("  Debounce:     {} ms", debounce);
    eprintln!("  Polling rate: {:?}", polling_rate);
    eprintln!("  LED mode:     {:?}", led_params.mode);
    eprintln!("  Brightness:   {}", led_params.brightness);
    eprintln!("  KB options:   {:?}", kb_options);
    eprintln!("  Precision:    {:?}", precision);
    eprintln!("  Sleep time:   {:?}", sleep_time);
}

/// Test the gRPC-style raw transport path: send_report → send_flush → read_report.
///
/// The gRPC server's `read_response()` uses this pattern instead of
/// FlowControlTransport. Verifies the raw path works for GET_USB_VERSION.
#[test]
#[ignore] // requires hardware
fn grpc_raw_transport_query() {
    let (raw, _kb) = open_keyboard();

    const GET_USB_VERSION: u8 = 0x8F;

    raw.send_report(GET_USB_VERSION, &[], ChecksumType::Bit7)
        .expect("send GET_USB_VERSION failed");
    raw.send_flush().ok();
    std::thread::sleep(Duration::from_millis(5));
    let result = raw.read_report().expect("Raw transport query failed");

    assert!(!result.is_empty(), "response should not be empty");
    assert_eq!(
        result[0], GET_USB_VERSION,
        "response should echo command byte"
    );
    assert!(result.len() >= 5, "response should have device_id bytes");

    let device_id = u32::from_le_bytes([result[1], result[2], result[3], result[4]]);
    eprintln!(
        "gRPC raw query: device_id = {} (0x{:08x})",
        device_id, device_id
    );
}

/// Probe macro support: raw GET_MACRO (0x8B) at the transport level.
///
/// Sends GET_MACRO for page 0 of slot 0 and prints whatever comes back.
/// This diagnoses whether the device responds at all.
#[test]
#[ignore] // requires hardware
fn macro_raw_probe() {
    let (raw, _kb) = open_keyboard();

    const GET_MACRO: u8 = 0x8B;

    // First: verify device is alive with a known-good command
    eprintln!("--- Verifying device connectivity with GET_USB_VERSION ---");
    raw.send_report(0x8F, &[], ChecksumType::Bit7)
        .expect("send GET_USB_VERSION failed");
    raw.send_flush().ok();
    std::thread::sleep(Duration::from_millis(10));
    match raw.read_report() {
        Ok(resp) => {
            eprintln!(
                "  GET_USB_VERSION response: [{:02x?}] (len={})",
                &resp[..resp.len().min(16)],
                resp.len()
            );
            assert_eq!(resp[0], 0x8F, "should echo 0x8F");
        }
        Err(e) => panic!("Device not responding to GET_USB_VERSION: {e}"),
    }

    // Now probe GET_MACRO
    eprintln!("\n--- Probing GET_MACRO (0x8B) for slot 0, page 0 ---");
    let data = [0u8, 0u8]; // macro_index=0, page=0
    raw.send_report(GET_MACRO, &data, ChecksumType::Bit7)
        .expect("send GET_MACRO failed");
    raw.send_flush().ok();

    // Try reading with increasing timeouts
    for delay in [10, 50, 200, 500] {
        std::thread::sleep(Duration::from_millis(delay));
        match raw.read_report() {
            Ok(resp) => {
                eprintln!("  GET_MACRO response after {delay}ms:");
                eprintln!("    len={}, first_byte=0x{:02x}", resp.len(), resp[0]);
                for (i, chunk) in resp.chunks(16).enumerate() {
                    eprint!("    {:04x}: ", i * 16);
                    for b in chunk {
                        eprint!("{b:02x} ");
                    }
                    eprintln!();
                }
                return; // success
            }
            Err(e) => {
                eprintln!("  No response after {delay}ms: {e}");
            }
        }
    }

    panic!("GET_MACRO: no response from device after all retries");
}

/// Read an existing macro (slot 0) and verify the high-level API parses it.
///
/// Slot 0 should have data from a previous set (either webapp or CLI).
#[test]
#[ignore] // requires hardware
fn macro_read_existing() {
    let (_raw, kb) = open_keyboard();

    eprintln!("--- Reading existing macro from slot 0 ---");
    for slot in 0..8u8 {
        eprint!("Slot {slot}: ");
        match kb.get_macro(slot) {
            Ok(data) => {
                // Check if data is all 0xFF (uninitialized)
                if data.iter().all(|&b| b == 0xFF) {
                    eprintln!("uninitialized (all 0xFF)");
                    continue;
                }
                let (repeat_count, events) = monsgeek_keyboard::parse_macro_events(&data);
                eprintln!(
                    "{} events, repeat={}, raw_len={}",
                    events.len(),
                    repeat_count,
                    data.len()
                );
                if !events.is_empty() {
                    for (i, evt) in events.iter().enumerate() {
                        let dir = if evt.is_down { "↓" } else { "↑" };
                        eprintln!(
                            "    {i}: {dir} keycode=0x{:02x} delay={}ms",
                            evt.keycode, evt.delay_ms
                        );
                    }
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
            }
        }
    }
}

/// Test SET_MACRO round-trip at the raw transport level.
///
/// Sends SET_MACRO for slot 7, waits, then reads it back with GET_MACRO.
#[test]
#[ignore] // requires hardware
fn macro_set_and_readback() {
    let (raw, _kb) = open_keyboard();

    eprintln!("--- SET_MACRO raw test for slot 7 ---");

    // Build a minimal macro: repeat=1, one keystroke 'a' down+up
    let mut macro_data = Vec::new();
    macro_data.push(0x01); // repeat count low
    macro_data.push(0x00); // repeat count high
    macro_data.push(0x04); // 'a' keycode
    macro_data.push(0x8A); // down + 10ms delay
    macro_data.push(0x04); // 'a' keycode
    macro_data.push(0x0A); // up + 10ms delay
                           // Pad to 56 bytes
    while macro_data.len() < 56 {
        macro_data.push(0);
    }

    // Page 0, is_last=1
    let mut cmd_data = vec![7u8, 0, 56, 1, 0, 0, 0];
    cmd_data.extend_from_slice(&macro_data);

    eprintln!("  Sending SET_MACRO (0x0B) for slot 7, page 0...");
    eprintln!(
        "  cmd_data[..16]: {:02x?}",
        &cmd_data[..cmd_data.len().min(16)]
    );
    raw.send_report(0x0B, &cmd_data, ChecksumType::Bit7)
        .expect("send SET_MACRO failed");
    raw.send_flush().ok();

    // Wait for device to process
    std::thread::sleep(Duration::from_millis(200));

    // Try to read any response/ack
    eprintln!("  Checking for SET_MACRO ack...");
    match raw.read_report() {
        Ok(resp) => {
            eprintln!(
                "  SET_MACRO response: first_byte=0x{:02x} len={}",
                resp[0],
                resp.len()
            );
            eprint!("    ");
            for b in resp.iter().take(16) {
                eprint!("{b:02x} ");
            }
            eprintln!();
        }
        Err(_) => eprintln!("  No ack — fire-and-forget is normal"),
    }

    // Now read it back with GET_MACRO
    eprintln!("\n  Reading back slot 7 with GET_MACRO (0x8B)...");
    let query_data = [7u8, 0u8]; // slot 7, page 0
    raw.send_report(0x8B, &query_data, ChecksumType::Bit7)
        .expect("send GET_MACRO failed");
    raw.send_flush().ok();
    std::thread::sleep(Duration::from_millis(50));

    match raw.read_report() {
        Ok(resp) => {
            eprintln!(
                "  GET_MACRO slot 7 response: first_byte=0x{:02x} len={}",
                resp[0],
                resp.len()
            );
            for (i, chunk) in resp.chunks(16).enumerate() {
                eprint!("    {:04x}: ", i * 16);
                for b in chunk {
                    eprint!("{b:02x} ");
                }
                eprintln!();
            }

            if resp.iter().all(|&b| b == 0xFF) {
                eprintln!("  RESULT: Still all 0xFF — SET_MACRO did not persist");
            } else if resp[0] == 0x01 && resp[1] == 0x00 {
                eprintln!("  RESULT: Data looks correct (repeat=1)");
            } else {
                eprintln!("  RESULT: Data present but unexpected format");
            }
        }
        Err(e) => {
            eprintln!("  GET_MACRO read failed: {e}");
        }
    }

    // Also try writing to slot 0 (which we know has data) and see if overwrite works
    eprintln!("\n--- SET_MACRO overwrite test for slot 0 ---");
    // Read slot 0 first
    raw.send_report(0x8B, &[0u8, 0u8], ChecksumType::Bit7)
        .expect("send GET_MACRO failed");
    raw.send_flush().ok();
    std::thread::sleep(Duration::from_millis(50));
    match raw.read_report() {
        Ok(resp) => {
            eprint!("  Slot 0 before write: ");
            for b in resp.iter().take(16) {
                eprint!("{b:02x} ");
            }
            eprintln!();
        }
        Err(e) => eprintln!("  Read error: {e}"),
    }

    // Write to slot 0
    let mut macro_data_0 = Vec::new();
    macro_data_0.push(0x01); // repeat count low
    macro_data_0.push(0x00); // repeat count high
    macro_data_0.push(0x17); // 't' keycode
    macro_data_0.push(0x8A); // down + 10ms delay
    macro_data_0.push(0x17); // 't' keycode
    macro_data_0.push(0x0A); // up + 10ms delay
    while macro_data_0.len() < 56 {
        macro_data_0.push(0);
    }

    let mut cmd_data_0 = vec![0u8, 0, 56, 1, 0, 0, 0];
    cmd_data_0.extend_from_slice(&macro_data_0);

    eprintln!("  Sending SET_MACRO for slot 0...");
    raw.send_report(0x0B, &cmd_data_0, ChecksumType::Bit7)
        .expect("send SET_MACRO failed");
    raw.send_flush().ok();
    std::thread::sleep(Duration::from_millis(200));

    // Drain any ack
    let _ = raw.read_report();

    // Read back slot 0
    raw.send_report(0x8B, &[0u8, 0u8], ChecksumType::Bit7)
        .expect("send GET_MACRO failed");
    raw.send_flush().ok();
    std::thread::sleep(Duration::from_millis(50));
    match raw.read_report() {
        Ok(resp) => {
            eprint!("  Slot 0 after write:  ");
            for b in resp.iter().take(16) {
                eprint!("{b:02x} ");
            }
            eprintln!();

            if resp[0] == 0x01 && resp[1] == 0x00 && resp[2] == 0x17 {
                eprintln!("  RESULT: Overwrite succeeded!");
            } else {
                eprintln!("  RESULT: Overwrite may not have worked");
            }
        }
        Err(e) => eprintln!("  Read error: {e}"),
    }
}

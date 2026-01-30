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
async fn open_keyboard() -> (Arc<dyn Transport>, KeyboardInterface) {
    let discovery = HidDiscovery::new();
    let transport = discovery
        .open_preferred()
        .await
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
    let (_raw, kb) = open_keyboard().await;
    let kb = Arc::new(kb);

    // Same queries as tui.rs load_device_info(), each spawned as a
    // separate task — exactly how the TUI does it
    let h_device_id = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_device_id().await })
    };
    let h_version = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_version().await })
    };
    let h_profile = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_profile().await })
    };
    let h_debounce = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_debounce().await })
    };
    let h_poll_rate = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_polling_rate().await })
    };
    let h_led = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_led_params().await })
    };
    let h_side_led = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_side_led_params().await })
    };
    let h_kb_opts = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_kb_options().await })
    };
    let h_precision = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_precision().await })
    };
    let h_sleep = {
        let kb = Arc::clone(&kb);
        tokio::spawn(async move { kb.get_sleep_time().await })
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
#[tokio::test]
#[ignore] // requires hardware
async fn grpc_raw_transport_query() {
    let (raw, _kb) = open_keyboard().await;

    const GET_USB_VERSION: u8 = 0x8F;

    let result = tokio::time::timeout(Duration::from_secs(5), async {
        raw.send_report(GET_USB_VERSION, &[], ChecksumType::Bit7)
            .await?;
        raw.send_flush().await?;
        tokio::time::sleep(Duration::from_millis(5)).await;
        raw.read_report().await
    })
    .await
    .expect("gRPC raw query did not complete within 5 seconds")
    .expect("Raw transport query failed");

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

//! Replay tests driving `BeiyingKeyboard` against captured USB fixture bytes.

use std::path::Path;

use monsgeek_beiying::{
    BeiyingKeyboard, REPORT_LEN, ReplayIo, apply_checksum, parse_firmware_version_reply,
    parse_polling_rate_reply, parse_profile_reply, verify_checksum,
};
use monsgeek_keyboard::{FirmwareVersion, Keyboard, KeyboardError, SleepTimeSettings};
use monsgeek_transport::protocol::Profile;
use monsgeek_transport::{TransportDeviceInfo, TransportType};

fn decode_hex_bytes(hex: &str) -> Vec<u8> {
    let clean = hex.trim();
    assert_eq!(clean.len() % 2, 0, "odd hex length");
    clean
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let hi = (pair[0] as char).to_digit(16).expect("valid hex digit");
            let lo = (pair[1] as char).to_digit(16).expect("valid hex digit");
            ((hi << 4) | lo) as u8
        })
        .collect()
}

fn parse_hid_reports(text: &str) -> Vec<Vec<u8>> {
    let mut hid_reports = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(hex) = trimmed.strip_prefix("HID Data:") {
            hid_reports.push(decode_hex_bytes(hex));
        }
    }
    hid_reports
}

fn parse_connect_fixture() -> (Vec<u8>, Vec<Vec<u8>>) {
    let text = include_str!("fixtures/beiying-connect.txt");
    let mut device_descriptor = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if device_descriptor.is_none()
            && let Some(hex) = trimmed.strip_prefix("CONTROL response data:")
        {
            device_descriptor = Some(decode_hex_bytes(hex));
            break;
        }
    }

    (
        device_descriptor.expect("Frame 1 CONTROL response data present"),
        parse_hid_reports(text),
    )
}

fn assert_fixture_valid(pcapng_name: &str, txt_str: &str, expected_count: usize) -> Vec<Vec<u8>> {
    let pcapng = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(pcapng_name);
    let meta = std::fs::metadata(&pcapng)
        .unwrap_or_else(|e| panic!("{pcapng_name} fixture must exist: {e}"));
    assert!(meta.len() > 0, "{pcapng_name} must not be empty");

    let reports = parse_hid_reports(txt_str);
    assert_eq!(
        reports.len(),
        expected_count,
        "expected {expected_count} captured reports in {pcapng_name}"
    );
    for (idx, report) in reports.iter().enumerate() {
        assert_eq!(
            report.len(),
            REPORT_LEN,
            "{pcapng_name} report #{idx} must be 64 bytes"
        );
        assert!(
            verify_checksum(report),
            "{pcapng_name} report #{idx} failed checksum"
        );
    }
    reports
}

fn fixture_device_info() -> TransportDeviceInfo {
    TransportDeviceInfo {
        vid: 0x38EE,
        pid: 0x0016,
        is_dongle: false,
        transport_type: TransportType::HidWired,
        device_path: String::new(),
        serial: None,
        product_name: Some("TAC75 HE".into()),
    }
}

#[test]
fn all_pcapng_fixtures_exist_and_all_txt_reports_have_valid_checksums() {
    let (device_desc, hid_reports) = parse_connect_fixture();
    assert_eq!(device_desc.len(), 18, "USB device descriptor is 18 bytes");
    assert_eq!(u16::from_le_bytes([device_desc[8], device_desc[9]]), 0x38EE);
    assert_eq!(
        u16::from_le_bytes([device_desc[10], device_desc[11]]),
        0x0016
    );
    assert_eq!(
        u16::from_le_bytes([device_desc[12], device_desc[13]]),
        0x0108
    );

    let connect = assert_fixture_valid(
        "beiying-connect.pcapng",
        include_str!("fixtures/beiying-connect.txt"),
        20,
    );
    assert_eq!(connect, hid_reports);

    assert_fixture_valid(
        "get-profile.pcapng",
        include_str!("fixtures/get-profile.txt"),
        4,
    );
    assert_fixture_valid(
        "get-profile-p2.pcapng",
        include_str!("fixtures/get-profile-p2.txt"),
        4,
    );
    assert_fixture_valid(
        "get-polling-rate.pcapng",
        include_str!("fixtures/get-polling-rate.txt"),
        6,
    );
    assert_fixture_valid(
        "get-polling-rate-codes.pcapng",
        include_str!("fixtures/get-polling-rate-codes.txt"),
        28,
    );
    assert_fixture_valid(
        "get-debounce.pcapng",
        include_str!("fixtures/get-debounce.txt"),
        4,
    );
    assert_fixture_valid(
        "get-sleep-time.pcapng",
        include_str!("fixtures/get-sleep-time.txt"),
        4,
    );
}

#[test]
fn replay_connect_and_get_version_match_captured_bytes_exactly() {
    let (device_desc, hid_reports) = parse_connect_fixture();
    let expected_bcd_device = u16::from_le_bytes([device_desc[12], device_desc[13]]);

    // Frames 7/9 (0x82 0x01 UUID) and Frames 11/13 (0x82 0x02 FW version).
    let req_uuid = &hid_reports[0];
    let resp_uuid = hid_reports[1].clone();
    let req_fw = &hid_reports[2];
    let resp_fw = hid_reports[3].clone();

    let io = ReplayIo::new(vec![resp_uuid, resp_fw]);
    let kb = BeiyingKeyboard::connect(fixture_device_info(), io)
        .expect("connect handshake succeeds against captured Frame 7/9");

    assert_eq!(kb.uuid(), &[0x11, 0x00, 0x00, 0x00, 0x00, 0x59]);
    assert_eq!(kb.uuid_hex(), "110000000059");

    let version = kb
        .get_version()
        .expect("get_version succeeds against captured Frame 11/13");
    assert_eq!(version.raw, expected_bcd_device);
    assert_eq!(version.raw, 0x0108);
    assert_eq!(version.format(), "v108");

    let sent = kb.io().sent_reports();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].as_slice(), req_uuid.as_slice());
    assert_eq!(sent[1].as_slice(), req_fw.as_slice());
}

#[test]
fn firmware_version_parser_accepts_populated_length_byte() {
    let mut report = [0u8; REPORT_LEN];
    report[0] = 0x09;
    report[1] = 0x82;
    report[2] = 0x02;
    report[4] = 0x01;
    report[5] = 0x00;
    report[6] = 0x02;
    report[7] = 0x08;
    report[8] = 0x01;
    report[63] = 0x66;

    let version = parse_firmware_version_reply(&report).expect("len=2 reply must parse");
    assert_eq!(version.raw, FirmwareVersion::new(0x0108).raw);
    assert_eq!(version.format(), "v108");
}

#[test]
fn replay_get_polling_rate_matches_captured_bytes_exactly() {
    let reports = parse_hid_reports(include_str!("fixtures/get-polling-rate.txt"));
    let req_uuid = &reports[0];
    let resp_uuid = reports[1].clone();
    // Frames 15 / 17 in trimmed get-polling-rate.txt (demo.hubx.pro raw frames 3707 / 3709).
    let req_rate = &reports[4];
    let resp_rate = reports[5].clone();

    let io = ReplayIo::new(vec![resp_uuid, resp_rate]);
    let kb = BeiyingKeyboard::connect(fixture_device_info(), io)
        .expect("connect handshake succeeds against get-polling-rate.txt");

    let hz = kb
        .get_polling_rate()
        .expect("get_polling_rate succeeds against captured 0x84 0x17 reply");
    assert_eq!(hz, 4000);

    let sent = kb.io().sent_reports();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].as_slice(), req_uuid.as_slice());
    assert_eq!(sent[1].as_slice(), req_rate.as_slice());
}

#[test]
fn replay_get_polling_rate_supports_all_seven_captured_codes_and_rejects_invalid_inputs() {
    let captured_reports = parse_hid_reports(include_str!("fixtures/get-polling-rate-codes.txt"));
    // Ordered as merged from individual labelled captures in `tests/fixtures/get-polling-rate-codes.txt`:
    // 125 Hz (0x03), 250 Hz (0x02), 500 Hz (0x01), 1000 Hz (0x00),
    // 2000 Hz (0x06), 4000 Hz (0x05), 8000 Hz (0x04).
    let expected_codes = [
        (0x03u8, 0x56u8, 125u16),
        (0x02u8, 0x57u8, 250u16),
        (0x01u8, 0x58u8, 500u16),
        (0x00u8, 0x59u8, 1000u16),
        (0x06u8, 0x53u8, 2000u16),
        (0x05u8, 0x54u8, 4000u16),
        (0x04u8, 0x55u8, 8000u16),
    ];
    let chunks: Vec<&[Vec<u8>]> = captured_reports.chunks_exact(4).collect();
    assert_eq!(chunks.len(), expected_codes.len());
    assert_eq!(captured_reports.len(), expected_codes.len() * 4);

    // Replay all 7 post-reconnect read-only 0x84 0x17 request/reply pairs across
    // USB device addresses 30..=36 (strictly excluding 0x04 0x17 write packets).
    for (chunk, &(code, expected_checksum, expected_hz)) in chunks.iter().zip(expected_codes.iter())
    {
        let uuid_resp = &chunk[1];
        let rate_req = &chunk[2];
        let rate_resp = &chunk[3];

        assert_eq!(&rate_req[..3], &[0x09, 0x84, 0x17]);
        assert_eq!(rate_req[6], 0x00);
        assert_eq!(&rate_resp[..7], &[0x09, 0x84, 0x17, 0x00, 0x01, 0x00, 0x01]);
        assert_eq!(rate_resp[7], code);
        assert_eq!(rate_resp[REPORT_LEN - 1], expected_checksum);
        assert!(
            verify_checksum(rate_resp),
            "captured 0x84 0x17 code 0x{code:02X} checksum 0x{expected_checksum:02X} must validate"
        );

        let io = ReplayIo::new(vec![uuid_resp.clone(), rate_resp.clone()]);
        let kb = BeiyingKeyboard::connect(fixture_device_info(), io)
            .expect("connect handshake succeeds for captured rate code replay");
        assert_eq!(kb.uuid_hex(), "110000000059");

        let hz = kb
            .get_polling_rate()
            .unwrap_or_else(|e| panic!("code 0x{code:02X} ({expected_hz} Hz) failed: {e}"));
        assert_eq!(hz, expected_hz);

        let sent = kb.io().sent_reports();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1].as_slice(), rate_req.as_slice());
    }

    let base_reports = parse_hid_reports(include_str!("fixtures/get-polling-rate.txt"));
    for uncaptured in [0x07u8, 0xFFu8] {
        let mut bad_code: [u8; REPORT_LEN] =
            base_reports[5].clone().try_into().expect("64-byte reply");
        bad_code[7] = uncaptured;
        apply_checksum(&mut bad_code);
        assert!(
            matches!(
                parse_polling_rate_reply(&bad_code),
                Err(KeyboardError::UnexpectedResponse(_))
            ),
            "uncaptured polling rate code 0x{uncaptured:02X} must be rejected"
        );
    }

    let mut bad_len: [u8; REPORT_LEN] = base_reports[5].clone().try_into().expect("64-byte reply");
    bad_len[6] = 0x00;
    bad_len[7] = 0x00;
    apply_checksum(&mut bad_len);
    assert!(matches!(
        parse_polling_rate_reply(&bad_len),
        Err(KeyboardError::UnexpectedResponse(_))
    ));
}

#[test]
fn replay_get_profile_matches_captured_bytes_for_profile_1_and_profile_2() {
    let p1_reports = parse_hid_reports(include_str!("fixtures/get-profile.txt"));
    let p2_reports = parse_hid_reports(include_str!("fixtures/get-profile-p2.txt"));

    // Profile 1 (`pkt[7] = 0x00` -> Profile(0), UI Profile 1) from fixtures/get-profile.txt.
    // Note: get-profile.txt retained the heartbeat 0x82 0x01 query (`pkt[6] = 0x06`), whereas
    // `BeiyingKeyboard::connect` sends `pkt[6] = 0x00` (matching get-profile-p2.txt Frame 5).
    let io_p1 = ReplayIo::new(vec![p1_reports[1].clone(), p1_reports[3].clone()]);
    let kb_p1 = BeiyingKeyboard::connect(fixture_device_info(), io_p1)
        .expect("connect handshake succeeds against get-profile.txt");
    let profile_1 = kb_p1
        .get_profile()
        .expect("get_profile succeeds for captured Profile 1 reply");
    assert_eq!(profile_1, Profile::ALL[0]);
    assert_eq!(profile_1.get(), 0);
    assert_eq!(profile_1.number(), 1);
    let sent_p1 = kb_p1.io().sent_reports();
    assert_eq!(sent_p1.len(), 2);
    assert_eq!(&sent_p1[0][..6], &p1_reports[0][..6]);
    assert_eq!(sent_p1[1].as_slice(), p1_reports[2].as_slice());

    // Profile 2 (`pkt[7] = 0x01` -> Profile(1), UI Profile 2) from fixtures/get-profile-p2.txt.
    let io_p2 = ReplayIo::new(vec![p2_reports[1].clone(), p2_reports[3].clone()]);
    let kb_p2 = BeiyingKeyboard::connect(fixture_device_info(), io_p2)
        .expect("connect handshake succeeds against get-profile-p2.txt");
    let profile_2 = kb_p2
        .get_profile()
        .expect("get_profile succeeds for captured Profile 2 reply");
    assert_eq!(profile_2, Profile::ALL[1]);
    assert_eq!(profile_2.get(), 1);
    assert_eq!(profile_2.number(), 2);
    let sent_p2 = kb_p2.io().sent_reports();
    assert_eq!(sent_p2.len(), 2);
    assert_eq!(sent_p2[0].as_slice(), p2_reports[0].as_slice());
    assert_eq!(sent_p2[1].as_slice(), p2_reports[2].as_slice());

    // Accept populated length `pkt[6] = 0x01` as well (e.g. Profile 3 -> `0x02`).
    let mut populated_len: [u8; REPORT_LEN] = p2_reports[3].clone().try_into().expect("64-byte");
    populated_len[6] = 0x01;
    populated_len[7] = 0x02;
    apply_checksum(&mut populated_len);
    let profile_3 = parse_profile_reply(&populated_len).expect("len=1 profile reply must parse");
    assert_eq!(profile_3, Profile::ALL[2]);
    assert_eq!(profile_3.number(), 3);

    // Reject out-of-range profile slot (>= 4).
    let mut bad_slot: [u8; REPORT_LEN] = p2_reports[3].clone().try_into().expect("64-byte");
    bad_slot[7] = 0x04;
    apply_checksum(&mut bad_slot);
    assert!(matches!(
        parse_profile_reply(&bad_slot),
        Err(KeyboardError::UnexpectedResponse(_))
    ));

    // Reject invalid declared payload length (> 1).
    let mut bad_len: [u8; REPORT_LEN] = p2_reports[3].clone().try_into().expect("64-byte");
    bad_len[6] = 0x02;
    apply_checksum(&mut bad_len);
    assert!(matches!(
        parse_profile_reply(&bad_len),
        Err(KeyboardError::UnexpectedResponse(_))
    ));
}

#[test]
fn unverified_queries_and_all_writes_return_not_supported_without_io() {
    let (_device_desc, hid_reports) = parse_connect_fixture();

    let debounce_reports = parse_hid_reports(include_str!("fixtures/get-debounce.txt"));
    let debounce_reply = &debounce_reports[3];
    assert_eq!(&debounce_reply[..3], &[0x09, 0x84, 0x1e]);
    assert_eq!(
        debounce_reply[6], 0,
        "get-debounce.txt returned zero payload bytes (unproven)"
    );

    let sleep_reports = parse_hid_reports(include_str!("fixtures/get-sleep-time.txt"));
    let sleep_reply = &sleep_reports[3];
    assert_eq!(&sleep_reply[..3], &[0x09, 0x84, 0x13]);
    assert_eq!(sleep_reply[6], 2);
    assert_eq!(&sleep_reply[7..9], &[0x00, 0xb4]);

    let io = ReplayIo::new(vec![hid_reports[1].clone()]);
    let kb = BeiyingKeyboard::connect(fixture_device_info(), io).expect("connect replay succeeds");

    assert!(matches!(
        kb.get_debounce(),
        Err(KeyboardError::NotSupported(_))
    ));
    assert!(matches!(
        kb.get_sleep_time(),
        Err(KeyboardError::NotSupported(_))
    ));

    assert!(matches!(
        kb.set_profile(Profile::ALL[0]),
        Err(KeyboardError::NotSupported(_))
    ));
    assert!(matches!(
        kb.set_polling_rate(1000),
        Err(KeyboardError::NotSupported(_))
    ));
    assert!(matches!(
        kb.set_debounce(5),
        Err(KeyboardError::NotSupported(_))
    ));
    assert!(matches!(
        kb.set_sleep_time(&SleepTimeSettings::default()),
        Err(KeyboardError::NotSupported(_))
    ));
    assert!(matches!(kb.reset(), Err(KeyboardError::NotSupported(_))));

    assert_eq!(
        kb.io().sent_reports().len(),
        1,
        "only the connect handshake may write reports for unsupported commands"
    );
}

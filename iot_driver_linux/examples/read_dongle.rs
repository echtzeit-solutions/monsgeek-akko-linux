// Dongle status buffer monitor - watches for changes
use std::time::{Duration, Instant};

fn main() {
    let api = hidapi::HidApi::new().expect("HID init");

    println!("Looking for dongle (3151:5038)...\n");

    for dev in api.device_list() {
        if dev.vendor_id() == 0x3151
            && dev.product_id() == 0x5038
            && dev.usage_page() == 0xFFFF
            && dev.usage() == 0x01
        {
            println!("Found dongle. Monitoring for 60 seconds...");
            println!("Try: press keys, move keyboard away, turn keyboard off\n");

            if let Ok(h) = dev.open_device(&api) {
                let mut last = [0u8; 65];
                let mut first = true;
                let start = Instant::now();

                while start.elapsed() < Duration::from_secs(60) {
                    let mut buf = [0u8; 65];
                    buf[0] = 0x05;

                    if h.get_feature_report(&mut buf).is_ok() {
                        // Check if anything changed
                        let changed = buf[..8] != last[..8];

                        if first || changed {
                            let elapsed = start.elapsed().as_secs_f32();
                            println!("[{:5.1}s] bat={:3}% chrg={} online={} | {:02x} {:02x} {:02x} {:02x}{}",
                                elapsed,
                                buf[1],
                                buf[2],
                                buf[3],
                                buf[4], buf[5], buf[6], buf[7],
                                if changed && !first { " <-- CHANGED!" } else { "" });

                            // Show which bytes changed
                            if changed && !first {
                                for i in 0..8 {
                                    if buf[i] != last[i] {
                                        println!(
                                            "        byte[{}]: {:02x} -> {:02x}",
                                            i, last[i], buf[i]
                                        );
                                    }
                                }
                            }

                            last[..8].copy_from_slice(&buf[..8]);
                            first = false;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                println!("\nDone monitoring.");
            }
            return;
        }
    }
    println!("Dongle not found");
}

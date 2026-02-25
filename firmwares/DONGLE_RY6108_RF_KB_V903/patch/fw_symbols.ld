/* Dongle firmware symbol addresses — manually curated for battery HID patch.
 * Link via: ld -T patch.ld -T fw_symbols.ld ... */

/* ── Firmware functions (Thumb, bit 0 set) ─────────────────────────────── */
memcpy                           = 0x080055f1;
usb_ep0_in_xfer_start           = 0x08009627;
usb_ep0_stall                   = 0x08009667;
usb_ep0_send_zlp                = 0x080095eb;
usb_otg_in_ep_xfer_start        = 0x08009aeb;

/* ── SRAM globals ──────────────────────────────────────────────────────── */
g_dongle_state                   = 0x20000330;
g_usb_device                     = 0x20000484;
g_if1_report_desc                = 0x200001EC;
g_ep2_report_buf                 = 0x200007F4;

/* Dongle firmware (dongle_working_256k.bin) extern header.
 * Manually written from Ghidra RE — only symbols needed for battery HID patch.
 * Link with fw_symbols.ld to resolve addresses. */
#ifndef FW_DONGLE_H
#define FW_DONGLE_H

#include <stdint.h>

/* ── Dongle state struct (333 bytes @ 0x20000330) ─────────────────────── */
/* Only fields used by the patch are declared. Full map in dongle-firmware-re.md. */
typedef struct __attribute__((packed)) {
    uint8_t usb_report_id;          /* +0x00 */
    uint8_t usb_response[64];       /* +0x01 */
    uint8_t vendor_cmd_pending;     /* +0x41 */
    uint8_t vendor_cmd_buf[64];     /* +0x42 */
    uint8_t ep1_in_xfer_busy;      /* +0x82  EP1 IN busy (cleared by SOF bit 0) */
    uint8_t ep2_in_xfer_busy;      /* +0x83  EP2 IN busy (cleared by SOF bit 1) */
    uint8_t _pad_84[0x54];         /* +0x84 .. +0xD7 */
    uint8_t rf_idle;                /* +0xD8 (idle status from keyboard) */
    uint8_t _pad_d9[2];            /* +0xD9 .. +0xDA */
    uint8_t kb_battery_info;        /* +0xDB */
    uint8_t kb_charging;            /* +0xDC */
    uint8_t kb_connection_status;   /* +0xDD */
} dongle_state_t;  /* partial, 222 bytes declared */

/* ── SPI buffer struct (150 bytes @ 0x20000834) ──────────────────────── */
/* Used to intercept RF packets before rf_packet_dispatch processes them. */
typedef struct __attribute__((packed)) {
    uint8_t _pad_00;                /* +0x00 */
    uint8_t tx_ready;               /* +0x01 */
    uint8_t rx_processed;           /* +0x02 */
    uint8_t _pad_03;                /* +0x03 */
    uint8_t rx_command;             /* +0x04  received cmd (0x81-0x8A, bit 7 = valid) */
    uint8_t rx_length;              /* +0x05 */
    uint8_t rx_data[70];            /* +0x06 */
    uint8_t rx_checksum;            /* +0x4C */
    uint8_t _pad_4d;                /* +0x4D */
    uint8_t tx_length;              /* +0x4E */
    uint8_t tx_command;             /* +0x4F */
    uint8_t tx_data[68];            /* +0x50 */
    uint8_t tx_checksum;            /* +0x95 */
} spi_buf_t;

/* ── Firmware globals (in SRAM, resolved by fw_symbols.ld) ────────────── */
extern dongle_state_t g_dongle_state;       /* 0x20000330 */
extern spi_buf_t g_spi_buf;                 /* 0x20000834 */
extern uint8_t g_usb_device[];              /* 0x20000484 (opaque USB device struct) */
extern uint8_t g_if1_report_desc[];         /* 0x200001EC (171 bytes, IF1 HID rdesc) */
extern uint8_t g_ep2_report_buf[];          /* 0x200007F4 (64 bytes, EP2 IN buffer) */

/* ── Firmware functions (Thumb, resolved by fw_symbols.ld) ────────────── */
extern void usb_ep0_in_xfer_start(void *udev, const void *buf, uint16_t len);
extern void usb_otg_in_ep_xfer_start(void *usb_dev, uint8_t ep, const void *buf, uint32_t len);
extern void *memcpy(void *dst, const void *src, unsigned int n);

#endif /* FW_DONGLE_H */

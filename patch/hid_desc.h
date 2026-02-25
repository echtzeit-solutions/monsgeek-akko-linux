/*
 * HID Report Descriptor builder macros.
 *
 * Extracted from TinyUSB (MIT license) with battery-specific additions.
 * Only the self-contained #define macros — no framework dependencies.
 *
 * Sources:
 *   tusb_common.h  — byte-splitting macros (U16_TO_U8S_LE, U32_TO_U8S_LE)
 *   class/hid/hid.h — report item builder + usage tables
 */

#ifndef HID_DESC_H
#define HID_DESC_H

#include <stdint.h>

/* ── Byte-splitting (from tusb_common.h) ──────────────────────────── */

#define TU_U16_HIGH(_u16)     ((uint8_t) (((_u16) >> 8) & 0x00ff))
#define TU_U16_LOW(_u16)      ((uint8_t) ((_u16)       & 0x00ff))
#define U16_TO_U8S_LE(_u16)   TU_U16_LOW(_u16), TU_U16_HIGH(_u16)

#define TU_U32_BYTE0(_u32)    ((uint8_t) (((uint32_t)  _u32)        & 0x000000ff))
#define TU_U32_BYTE1(_u32)    ((uint8_t) ((((uint32_t) _u32) >>  8) & 0x000000ff))
#define TU_U32_BYTE2(_u32)    ((uint8_t) ((((uint32_t) _u32) >> 16) & 0x000000ff))
#define TU_U32_BYTE3(_u32)    ((uint8_t) ((((uint32_t) _u32) >> 24) & 0x000000ff))
#define U32_TO_U8S_LE(_u32)   TU_U32_BYTE0(_u32), TU_U32_BYTE1(_u32), TU_U32_BYTE2(_u32), TU_U32_BYTE3(_u32)

/* ── Report item builder (from class/hid/hid.h) ──────────────────── */

#define HID_REPORT_DATA_0(data)
#define HID_REPORT_DATA_1(data) , data
#define HID_REPORT_DATA_2(data) , U16_TO_U8S_LE(data)
#define HID_REPORT_DATA_3(data) , U32_TO_U8S_LE(data)

#define HID_REPORT_ITEM(data, tag, type, size) \
  (((tag) << 4) | ((type) << 2) | (size)) HID_REPORT_DATA_##size(data)

/* Report item type codes */
#define RI_TYPE_MAIN    0
#define RI_TYPE_GLOBAL  1
#define RI_TYPE_LOCAL   2

/* ── Main items ───────────────────────────────────────────────────── */

#define RI_MAIN_INPUT           8
#define RI_MAIN_OUTPUT          9
#define RI_MAIN_COLLECTION      10
#define RI_MAIN_FEATURE         11
#define RI_MAIN_COLLECTION_END  12

#define HID_INPUT(x)           HID_REPORT_ITEM(x, RI_MAIN_INPUT,          RI_TYPE_MAIN, 1)
#define HID_OUTPUT(x)          HID_REPORT_ITEM(x, RI_MAIN_OUTPUT,         RI_TYPE_MAIN, 1)
#define HID_COLLECTION(x)      HID_REPORT_ITEM(x, RI_MAIN_COLLECTION,     RI_TYPE_MAIN, 1)
#define HID_FEATURE(x)         HID_REPORT_ITEM(x, RI_MAIN_FEATURE,        RI_TYPE_MAIN, 1)
#define HID_COLLECTION_END     HID_REPORT_ITEM(0, RI_MAIN_COLLECTION_END, RI_TYPE_MAIN, 0)

/* Input/Output/Feature data bits */
#define HID_DATA             (0<<0)
#define HID_CONSTANT         (1<<0)
#define HID_ARRAY            (0<<1)
#define HID_VARIABLE         (1<<1)
#define HID_ABSOLUTE         (0<<2)
#define HID_RELATIVE         (1<<2)

/* Collection types */
#define HID_COLLECTION_PHYSICAL     0
#define HID_COLLECTION_APPLICATION  1
#define HID_COLLECTION_LOGICAL      2

/* ── Global items ─────────────────────────────────────────────────── */

#define RI_GLOBAL_USAGE_PAGE    0
#define RI_GLOBAL_LOGICAL_MIN   1
#define RI_GLOBAL_LOGICAL_MAX   2
#define RI_GLOBAL_REPORT_SIZE   7
#define RI_GLOBAL_REPORT_ID     8
#define RI_GLOBAL_REPORT_COUNT  9

#define HID_USAGE_PAGE(x)         HID_REPORT_ITEM(x, RI_GLOBAL_USAGE_PAGE,   RI_TYPE_GLOBAL, 1)
#define HID_LOGICAL_MIN(x)        HID_REPORT_ITEM(x, RI_GLOBAL_LOGICAL_MIN,  RI_TYPE_GLOBAL, 1)
#define HID_LOGICAL_MAX(x)        HID_REPORT_ITEM(x, RI_GLOBAL_LOGICAL_MAX,  RI_TYPE_GLOBAL, 1)
#define HID_LOGICAL_MAX_N(x, n)   HID_REPORT_ITEM(x, RI_GLOBAL_LOGICAL_MAX,  RI_TYPE_GLOBAL, n)
#define HID_REPORT_SIZE(x)        HID_REPORT_ITEM(x, RI_GLOBAL_REPORT_SIZE,  RI_TYPE_GLOBAL, 1)
#define HID_REPORT_COUNT(x)       HID_REPORT_ITEM(x, RI_GLOBAL_REPORT_COUNT, RI_TYPE_GLOBAL, 1)

/* NOTE: HID_REPORT_ID has a trailing comma — TinyUSB convention for
 * array initializer usage (the Report ID item is always followed by
 * more items in the same collection). */
#define HID_REPORT_ID(x)          HID_REPORT_ITEM(x, RI_GLOBAL_REPORT_ID,    RI_TYPE_GLOBAL, 1),

/* ── Local items ──────────────────────────────────────────────────── */

#define RI_LOCAL_USAGE  0

#define HID_USAGE(x)              HID_REPORT_ITEM(x, RI_LOCAL_USAGE, RI_TYPE_LOCAL, 1)

/* ── Usage page constants ─────────────────────────────────────────── */

#define HID_USAGE_PAGE_DESKTOP          0x01
#define HID_USAGE_PAGE_GENERIC_DEVICE   0x06
#define HID_USAGE_PAGE_BATTERY_SYSTEM   0x85

/* ── Usage constants ──────────────────────────────────────────────── */

#define HID_USAGE_DESKTOP_KEYBOARD      0x06
#define HID_USAGE_BATTERY_STRENGTH      0x20  /* Generic Device Controls (0x06) */
#define HID_USAGE_BATTERY_CHARGING      0x44  /* Battery System (0x85) */

#endif /* HID_DESC_H */

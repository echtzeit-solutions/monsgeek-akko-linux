/* Auto-generated from Ghidra project 'firmware_2949_v408.bin'. Do not edit manually.
 * Extern header — link with fw_symbols.ld to resolve addresses.
 * For flash patches where BL can reach firmware code directly. */
#ifndef FW_V407_H
#define FW_V407_H

#include <stdint.h>
#include <stdbool.h>

/* ── Memory layout ─────────────────────────────────────────────────────── */
/* ram                   0x08005000 - 0x0802567f   132736B  rwx  init */
/* config_header         0x08028000 - 0x080287ff     2048B  rw   uninit */
/* keymaps               0x08028800 - 0x0802a7ff     8192B  rw   uninit */
/* fn_layers             0x0802a800 - 0x0802b7ff     4096B  rw   uninit */
/* macros                0x0802b800 - 0x0802efff    14336B  rw   uninit */
/* userpics              0x0802f800 - 0x0802ffff     2048B  rw   uninit */
/* calib_data            0x08032000 - 0x080337ff     6144B  rw   uninit */
/* magnetism             0x08033800 - 0x080377ff    16384B  rw   uninit */
/* SRAM                  0x20000000 - 0x20017fff    98304B  rw   uninit */

/* ── Data types (structs) ───────────────────────────────────────────────── */

typedef struct __attribute__((packed)) {
    uint8_t _pad_0x00[1];  /* offset 0x0, 1B */
} T_UINT32;  /* 1 bytes */
_Static_assert(sizeof(T_UINT32) == 1, "T_UINT32 size mismatch");

typedef struct {
    uint8_t dirty_flags;  /* offset 0x0, 1B */
    uint8_t config_sync_pending;  /* offset 0x1, 1B */
    uint8_t game_disable_mask[6];  /* offset 0x2, 6B */
    uint32_t key_disable_col_bitmap[5];  /* offset 0x8, 20B */
    uint8_t _pad_1c[56];  /* offset 0x1c, 56B */
} bt_flags_t;  /* 84 bytes */
_Static_assert(sizeof(bt_flags_t) == 84, "bt_flags_t size mismatch");

typedef struct {
    uint32_t ffdb1;  /* offset 0x0, 4B */
    uint32_t ffdb2;  /* offset 0x4, 4B */
} can_filter_register_type;  /* 8 bytes */
_Static_assert(sizeof(can_filter_register_type) == 8, "can_filter_register_type size mismatch");

typedef struct {
    uint8_t blink_toggle;  /* offset 0x0, 1B */
    uint8_t tick_counter;  /* offset 0x1, 1B */
    uint8_t blink_mode;  /* offset 0x2, 1B */
} conn_indicator_state_t;  /* 3 bytes */
_Static_assert(sizeof(conn_indicator_state_t) == 3, "conn_indicator_state_t size mismatch");

typedef struct {
    uint8_t profile_id;  /* offset 0x0, 1B */
    uint8_t response_code;  /* offset 0x1, 1B */
    uint8_t debounce_config;  /* offset 0x2, 1B */
    uint8_t key_mode_flags;  /* offset 0x3, 1B */
    uint8_t flags1;  /* offset 0x4, 1B */
    uint8_t flags2;  /* offset 0x5, 1B */
    uint8_t sleep_timeout;  /* offset 0x6, 1B */
    uint8_t bt_conn_param;  /* offset 0x7, 1B */
    uint8_t led_effect_mode;  /* offset 0x8, 1B */
    uint8_t led_param;  /* offset 0x9, 1B */
    uint8_t led_brightness;  /* offset 0xa, 1B */
    uint8_t device_conn_state;  /* offset 0xb, 1B */
    uint8_t config_0c;  /* offset 0xc, 1B */
    uint8_t led_display_config[7];  /* offset 0xd, 7B */
    uint8_t ext_param_14;  /* offset 0x14, 1B */
    uint8_t ext_param_15;  /* offset 0x15, 1B */
    int16_t ext_param_16;  /* offset 0x16, 2B */
    int16_t ext_param_18;  /* offset 0x18, 2B */
    int16_t ext_param_1a;  /* offset 0x1a, 2B */
    int16_t ext_param_1c;  /* offset 0x1c, 2B */
    uint8_t connection_mode;  /* offset 0x1e, 1B */
    uint8_t _pad_0x1f[195];  /* offset 0x1f, 195B */
    uint8_t config_mode;  /* offset 0xe2, 1B */
    uint8_t config_setting_1;  /* offset 0xe3, 1B */
    uint8_t redraw_dirty;  /* offset 0xe4, 1B */
    uint8_t _pad_0xe5[1];  /* offset 0xe5, 1B */
    uint8_t config_setting_2;  /* offset 0xe6, 1B */
    uint8_t _tail_pad[5];  /* offset 0xe7, 5B */
} connection_config_t;  /* 236 bytes */
_Static_assert(sizeof(connection_config_t) == 236, "connection_config_t size mismatch");

typedef struct {
    uint8_t report_type;  /* offset 0x0, 1B */
    uint8_t data[7];  /* offset 0x1, 7B */
} consumer_report_t;  /* 8 bytes */
_Static_assert(sizeof(consumer_report_t) == 8, "consumer_report_t size mismatch");

typedef struct {
    uint32_t sclk_freq;  /* offset 0x0, 4B */
    uint32_t ahb_freq;  /* offset 0x4, 4B */
    uint32_t apb2_freq;  /* offset 0x8, 4B */
    uint32_t apb1_freq;  /* offset 0xc, 4B */
} crm_clocks_freq_type;  /* 16 bytes */
_Static_assert(sizeof(crm_clocks_freq_type) == 16, "crm_clocks_freq_type size mismatch");

typedef struct {
    uint8_t flags;  /* offset 0x0, 1B */
    uint8_t _pad_01[3];  /* offset 0x1, 3B */
} dirty_flags_t;  /* 4 bytes */
_Static_assert(sizeof(dirty_flags_t) == 4, "dirty_flags_t size mismatch");

typedef struct {
    uint8_t layer_0_pressed[21];  /* offset 0x0, 21B */
    uint8_t layer_1_pressed[21];  /* offset 0x15, 21B */
    uint8_t layer_2_pressed[21];  /* offset 0x2a, 21B */
    uint8_t layer_3_pressed[21];  /* offset 0x3f, 21B */
} dks_layer_bitmap_t;  /* 84 bytes */
_Static_assert(sizeof(dks_layer_bitmap_t) == 84, "dks_layer_bitmap_t size mismatch");

typedef struct {
    uint8_t pending_6kro_report;  /* offset 0x0, 1B */
    uint8_t send_interval_counter;  /* offset 0x1, 1B */
    uint8_t min_interval_counter;  /* offset 0x2, 1B */
    uint8_t pending_consumer_report;  /* offset 0x3, 1B */
    uint8_t pending_battery_report;  /* offset 0x4, 1B */
    uint8_t pending_conn_mode;  /* offset 0x5, 1B */
    uint8_t pending_status_0x96;  /* offset 0x6, 1B */
    uint8_t pending_nkro_report;  /* offset 0x7, 1B */
    uint8_t pending_status_0x98;  /* offset 0x8, 1B */
    uint8_t pending_mouse_report;  /* offset 0x9, 1B */
    uint8_t pending_dial_report;  /* offset 0xa, 1B */
    uint8_t pending_extra_report;  /* offset 0xb, 1B */
    uint8_t _pad_0c;  /* offset 0xc, 1B */
    uint8_t pending_vendor_data_relay;  /* offset 0xd, 1B */
    uint8_t pending_bulk_data_send;  /* offset 0xe, 1B */
    uint8_t _pad_0f[2];  /* offset 0xf, 2B */
    uint8_t pending_large_data_relay;  /* offset 0x11, 1B */
} dongle_report_flags_t;  /* 18 bytes */
_Static_assert(sizeof(dongle_report_flags_t) == 18, "dongle_report_flags_t size mismatch");

typedef struct {
    uint8_t gpio_current;  /* offset 0x0, 1B */
    uint8_t gpio_previous;  /* offset 0x1, 1B */
    uint8_t quadrature_state;  /* offset 0x2, 1B */
    uint8_t cw_counter;  /* offset 0x3, 1B */
    uint8_t cw_active;  /* offset 0x4, 1B */
    uint8_t ccw_counter;  /* offset 0x5, 1B */
    uint8_t ccw_active;  /* offset 0x6, 1B */
    uint8_t rate_tick_ctr;  /* offset 0x7, 1B */
    uint8_t debounce_ctr;  /* offset 0x8, 1B */
    uint8_t direction;  /* offset 0x9, 1B */
    uint8_t btn_debounce_ctr;  /* offset 0xa, 1B */
    uint8_t transition_state;  /* offset 0xb, 1B */
} encoder_state_t;  /* 12 bytes */
_Static_assert(sizeof(encoder_state_t) == 12, "encoder_state_t size mismatch");

typedef struct {
    uint8_t report_type;  /* offset 0x0, 1B */
    uint8_t modifier_byte;  /* offset 0x1, 1B */
    uint8_t keys[6];  /* offset 0x2, 6B */
} hid_6kro_report_t;  /* 8 bytes */
_Static_assert(sizeof(hid_6kro_report_t) == 8, "hid_6kro_report_t size mismatch");

typedef struct {
    uint8_t pending_reports_bitmap;  /* offset 0x0, 1B */
    uint8_t _pad_01;  /* offset 0x1, 1B */
    uint8_t fn_combo_state;  /* offset 0x2, 1B */
    uint8_t game_combo_state;  /* offset 0x3, 1B */
    uint8_t _pad_04;  /* offset 0x4, 1B */
    uint8_t knob_prev_keycode;  /* offset 0x5, 1B */
    uint8_t knob_last_keycode;  /* offset 0x6, 1B */
    uint8_t knob_press_debounce;  /* offset 0x7, 1B */
    uint8_t knob_releasing_keycode;  /* offset 0x8, 1B */
    uint8_t knob_pending_release;  /* offset 0x9, 1B */
    uint8_t ep1_tx_ready;  /* offset 0xa, 1B */
    uint8_t ep2_tx_ready;  /* offset 0xb, 1B */
} hid_report_state_t;  /* 12 bytes */
_Static_assert(sizeof(hid_report_state_t) == 12, "hid_report_state_t size mismatch");

typedef struct {
    uint8_t scan_tick_counter;  /* offset 0x0, 1B */
    uint8_t report_state;  /* offset 0x1, 1B */
    uint8_t poll_counter_1;  /* offset 0x2, 1B */
    uint8_t poll_counter_2;  /* offset 0x3, 1B */
    uint8_t connection_flags;  /* offset 0x4, 1B */
    uint8_t feature_flags;  /* offset 0x5, 1B */
    uint8_t state_06;  /* offset 0x6, 1B */
    uint8_t state_07;  /* offset 0x7, 1B */
    uint8_t layer_index;  /* offset 0x8, 1B */
    uint8_t state_09;  /* offset 0x9, 1B */
    uint8_t _pad_0a[2];  /* offset 0xa, 2B */
    int32_t timeout_counter;  /* offset 0xc, 4B */
    uint8_t led_override_active;  /* offset 0x10, 1B */
    uint8_t _pad_11[2];  /* offset 0x11, 2B */
    uint8_t led_override_color_idx;  /* offset 0x13, 1B */
    uint8_t _pad_14[4];  /* offset 0x14, 4B */
    uint32_t usb_frame_number;  /* offset 0x18, 4B */
    int32_t control_word;  /* offset 0x1c, 4B */
    uint8_t report_byte_20;  /* offset 0x20, 1B */
    uint8_t rf_reconnect_pending;  /* offset 0x21, 1B */
    uint8_t config_byte_22;  /* offset 0x22, 1B */
    uint8_t report_byte_23;  /* offset 0x23, 1B */
    uint8_t conn_switch_target;  /* offset 0x24, 1B */
    uint8_t transition_tick_ctr;  /* offset 0x25, 1B */
    uint8_t dongle_paired;  /* offset 0x26, 1B */
    uint8_t status_byte;  /* offset 0x27, 1B */
    uint8_t isp_prepare_flag;  /* offset 0x28, 1B */
    uint8_t deep_sleep_request;  /* offset 0x29, 1B */
    uint8_t state_flag_2a;  /* offset 0x2a, 1B */
    uint8_t sleep_poll_counter;  /* offset 0x2b, 1B */
    uint32_t idle_counter;  /* offset 0x2c, 4B */
    uint8_t pairing_state;  /* offset 0x30, 1B */
    uint8_t debounce_counter;  /* offset 0x31, 1B */
    uint8_t state_32;  /* offset 0x32, 1B */
    uint8_t state_33;  /* offset 0x33, 1B */
    uint8_t _pad_34[4];  /* offset 0x34, 4B */
    uint32_t power_transition_ctr;  /* offset 0x38, 4B */
    uint32_t battery_poll_counter;  /* offset 0x3c, 4B */
    uint8_t battery_level;  /* offset 0x40, 1B */
    uint8_t charger_connected;  /* offset 0x41, 1B */
    uint8_t charger_debounce_ctr;  /* offset 0x42, 1B */
    uint8_t battery_update_ctr;  /* offset 0x43, 1B */
    uint8_t battery_raw_level;  /* offset 0x44, 1B */
    uint8_t animation_dirty;  /* offset 0x45, 1B */
    uint8_t battery_indicator_active;  /* offset 0x46, 1B */
    uint8_t low_batt_blink_state;  /* offset 0x47, 1B */
    int16_t low_batt_blink_ctr;  /* offset 0x48, 2B */
    uint8_t charge_detect_flag;  /* offset 0x4a, 1B */
    uint8_t charge_detect_ctr;  /* offset 0x4b, 1B */
    uint8_t charge_status_ctr1;  /* offset 0x4c, 1B */
    uint8_t charge_status;  /* offset 0x4d, 1B */
    uint8_t charge_status_ctr2;  /* offset 0x4e, 1B */
    uint8_t _pad_4f;  /* offset 0x4f, 1B */
} kbd_state_t;  /* 80 bytes */
_Static_assert(sizeof(kbd_state_t) == 80, "kbd_state_t size mismatch");

typedef struct {
    uint8_t _pad_00[5];  /* offset 0x0, 5B */
    uint8_t prev_scancode;  /* offset 0x5, 1B */
    uint8_t pending_press_code;  /* offset 0x6, 1B */
    uint8_t press_debounce_count;  /* offset 0x7, 1B */
    uint8_t pending_release_code;  /* offset 0x8, 1B */
    uint8_t release_scancode;  /* offset 0x9, 1B */
    uint8_t _pad_0a[18];  /* offset 0xa, 18B */
    uint32_t release_debounce_counter;  /* offset 0x1c, 4B */
} key_debounce_state_t;  /* 32 bytes */
_Static_assert(sizeof(key_debounce_state_t) == 32, "key_debounce_state_t size mismatch");

typedef struct {
    uint8_t remaining_blinks;  /* offset 0x0, 1B */
    uint8_t tick_counter;  /* offset 0x1, 1B */
    uint8_t active;  /* offset 0x2, 1B */
} led_effect_state_t;  /* 3 bytes */
_Static_assert(sizeof(led_effect_state_t) == 3, "led_effect_state_t size mismatch");

typedef struct {
    uint8_t remaining_blinks;  /* offset 0x0, 1B */
    uint8_t tick_counter;  /* offset 0x1, 1B */
    uint8_t active;  /* offset 0x2, 1B */
} led_single_flash_t;  /* 3 bytes */
_Static_assert(sizeof(led_single_flash_t) == 3, "led_single_flash_t size mismatch");

typedef struct {
    uint32_t bitrate;  /* offset 0x0, 4B */
    uint8_t format;  /* offset 0x4, 1B */
    uint8_t parity;  /* offset 0x5, 1B */
    uint8_t data;  /* offset 0x6, 1B */
    uint8_t _pad_0x07[1];  /* offset 0x7, 1B */
} linecoding_type;  /* 8 bytes */
_Static_assert(sizeof(linecoding_type) == 8, "linecoding_type size mismatch");

typedef struct {
    uint8_t active_layer;  /* offset 0x0, 1B */
    uint8_t macro_slot_id;  /* offset 0x1, 1B */
    uint8_t _pad_02[2];  /* offset 0x2, 2B */
    int32_t tick_countdown;  /* offset 0x4, 4B */
    int32_t page_position;  /* offset 0x8, 4B */
    int32_t remaining_events;  /* offset 0xc, 4B */
    uint8_t modifier_xor;  /* offset 0x10, 1B */
    uint8_t playback_active;  /* offset 0x11, 1B */
    uint8_t restart_needed;  /* offset 0x12, 1B */
    uint8_t last_macro_slot;  /* offset 0x13, 1B */
} macro_playback_state_t;  /* 20 bytes */
_Static_assert(sizeof(macro_playback_state_t) == 20, "macro_playback_state_t size mismatch");

typedef struct {
    uint8_t current_adc_value[126];  /* offset 0x0, 126B */
    uint16_t cal_actuation_threshold[126];  /* offset 0x7e, 252B */
    uint16_t cal_reset_threshold[126];  /* offset 0x17a, 252B */
    uint16_t cal_rapid_trigger_down[126];  /* offset 0x276, 252B */
    uint16_t cal_rapid_trigger_up[126];  /* offset 0x372, 252B */
    uint16_t cal_param_5[126];  /* offset 0x46e, 252B */
    uint8_t cal_byte_6[126];  /* offset 0x56a, 126B */
    uint8_t cal_byte_7[126];  /* offset 0x5e8, 126B */
    uint8_t cal_byte_8[126];  /* offset 0x666, 126B */
    uint8_t cal_byte_9[126];  /* offset 0x6e4, 126B */
    uint8_t _pad_0x762[378];  /* offset 0x762, 378B */
    uint8_t cal_byte_10[29];  /* offset 0x8dc, 29B */
    uint8_t cal_mode_1b;  /* offset 0x8f9, 1B */
    uint8_t _pad_0x8fa[1];  /* offset 0x8fa, 1B */
    uint8_t cal_mode_1c;  /* offset 0x8fb, 1B */
    uint8_t cal_mode_field;  /* offset 0x8fc, 1B */
    uint8_t auto_cal_mode;  /* offset 0x8fd, 1B */
    uint8_t switch_threshold_mode;  /* offset 0x8fe, 1B */
    uint8_t connection_mode_gpio;  /* offset 0x8ff, 1B */
    uint8_t _pad_0x900[65];  /* offset 0x900, 65B */
    uint8_t adc_scan_counter;  /* offset 0x941, 1B */
    uint8_t _pad_0x942[1];  /* offset 0x942, 1B */
    uint8_t flash_magic_55;  /* offset 0x943, 1B */
    uint8_t flash_magic_aa;  /* offset 0x944, 1B */
    uint8_t _pad_0x945[1241];  /* offset 0x945, 1241B */
    uint8_t sleep_wake_flag;  /* offset 0xe1e, 1B */
    uint8_t sleep_field_2;  /* offset 0xe1f, 1B */
    uint8_t clock_switch_flag;  /* offset 0xe20, 1B */
    uint8_t _pad_0xe21[163];  /* offset 0xe21, 163B */
    uint8_t mag_dirty_buf_1[126];  /* offset 0xec4, 126B */
    uint8_t mag_dirty_buf_2[126];  /* offset 0xf42, 126B */
    uint8_t _pad_0xfc0[1386];  /* offset 0xfc0, 1386B */
    uint8_t computed_param_a[126];  /* offset 0x152a, 126B */
    uint8_t computed_param_b[126];  /* offset 0x15a8, 126B */
    uint16_t cal_param_11[126];  /* offset 0x1626, 252B */
    uint8_t cal_byte_12[126];  /* offset 0x1722, 126B */
    uint8_t _pad_0x17a0[130];  /* offset 0x17a0, 130B */
    uint8_t key_disabled_flags[126];  /* offset 0x1822, 126B */
    uint8_t _pad_0x18a0[7072];  /* offset 0x18a0, 7072B */
    uint8_t dks_flash_buf_copy[252];  /* offset 0x3440, 252B */
    uint8_t _pad_0x353c[505];  /* offset 0x353c, 505B */
    uint8_t dks_switch_type[126];  /* offset 0x3735, 126B */
    uint8_t dks_switch_category[126];  /* offset 0x37b3, 126B */
    uint8_t _pad_0x3831[1517];  /* offset 0x3831, 1517B */
    uint8_t adc_recal_needed;  /* offset 0x3e1e, 1B */
    uint8_t _pad_0x3e1f[951];  /* offset 0x3e1f, 951B */
    uint16_t adc_filtered_value[126];  /* offset 0x41d6, 252B */
    uint16_t adc_debounced_value[126];  /* offset 0x42d2, 252B */
    uint8_t adc_recal_flag[126];  /* offset 0x43ce, 126B */
    uint8_t adc_debounce_count[126];  /* offset 0x444c, 126B */
    uint8_t _pad_0x44ca[1145];  /* offset 0x44ca, 1145B */
    uint8_t flash_sig_0;  /* offset 0x4943, 1B */
    uint8_t flash_sig_1;  /* offset 0x4944, 1B */
    uint8_t _tail_pad[3];  /* offset 0x4945, 3B */
} mag_engine_state_t;  /* 18760 bytes */
_Static_assert(sizeof(mag_engine_state_t) == 18760, "mag_engine_state_t size mismatch");

typedef struct {
    uint8_t cal_done;  /* offset 0x0, 1B */
    uint8_t auto_cal_done;  /* offset 0x1, 1B */
    uint8_t mag_config_dirty;  /* offset 0x2, 1B */
    uint8_t mag_dks_dirty;  /* offset 0x3, 1B */
    uint32_t current_scan_depth;  /* offset 0x4, 4B */
} mag_scan_state_t;  /* 8 bytes */
_Static_assert(sizeof(mag_scan_state_t) == 8, "mag_scan_state_t size mismatch");

typedef struct {
    uint8_t report_type;  /* offset 0x0, 1B */
    uint8_t buttons;  /* offset 0x1, 1B */
    int16_t x_delta;  /* offset 0x2, 2B */
    int16_t y_delta;  /* offset 0x4, 2B */
    char wheel;  /* offset 0x6, 1B */
    char pan;  /* offset 0x7, 1B */
} mouse_report_t;  /* 8 bytes */
_Static_assert(sizeof(mouse_report_t) == 8, "mouse_report_t size mismatch");

typedef struct {
    uint8_t report_type;  /* offset 0x0, 1B */
    uint8_t modifier_byte;  /* offset 0x1, 1B */
    uint8_t key_bitmap[32];  /* offset 0x2, 32B */
} nkro_report_t;  /* 34 bytes */
_Static_assert(sizeof(nkro_report_t) == 34, "nkro_report_t size mismatch");

typedef struct {
    uint8_t payload[80];  /* offset 0x0, 80B */
    uint8_t total_length;  /* offset 0x50, 1B */
    uint8_t tx_busy;  /* offset 0x51, 1B */
    uint8_t adc_packet_size;  /* offset 0x52, 1B */
    uint8_t _pad_53;  /* offset 0x53, 1B */
} rf_packet_buf_t;  /* 84 bytes */
_Static_assert(sizeof(rf_packet_buf_t) == 84, "rf_packet_buf_t size mismatch");

typedef struct {
    uint8_t tick_flag;  /* offset 0x0, 1B */
    uint8_t led_index;  /* offset 0x1, 1B */
    uint8_t row_index;  /* offset 0x2, 1B */
    uint8_t config_pending;  /* offset 0x3, 1B */
    uint32_t ripple_active_0;  /* offset 0x4, 4B */
    uint32_t ripple_active_1;  /* offset 0x8, 4B */
    uint32_t ripple_active_2;  /* offset 0xc, 4B */
    uint32_t ripple_active_3;  /* offset 0x10, 4B */
    uint32_t ripple_active_4;  /* offset 0x14, 4B */
    uint32_t ripple_active_5;  /* offset 0x18, 4B */
    uint8_t animation_dirty;  /* offset 0x1c, 1B */
    uint8_t wave_direction;  /* offset 0x1d, 1B */
    uint8_t color_mode;  /* offset 0x1e, 1B */
    uint8_t color_cycle_idx;  /* offset 0x1f, 1B */
    uint8_t brightness;  /* offset 0x20, 1B */
    uint8_t field_0x21;  /* offset 0x21, 1B */
    uint8_t computed_brightness;  /* offset 0x22, 1B */
    uint8_t speed_param;  /* offset 0x23, 1B */
    uint8_t speed_step;  /* offset 0x24, 1B */
    uint8_t field_0x25;  /* offset 0x25, 1B */
    uint8_t field_0x26;  /* offset 0x26, 1B */
    uint8_t field_0x27;  /* offset 0x27, 1B */
    uint32_t tick_counter;  /* offset 0x28, 4B */
    uint32_t effect_step_index;  /* offset 0x2c, 4B */
    int16_t delay_counter;  /* offset 0x30, 2B */
    int16_t field_0x32;  /* offset 0x32, 2B */
    uint32_t random_position;  /* offset 0x34, 4B */
    uint32_t last_position;  /* offset 0x38, 4B */
    uint8_t spiral_direction;  /* offset 0x3c, 1B */
    uint8_t effect_flip_flag;  /* offset 0x3d, 1B */
    uint8_t field_0x3e;  /* offset 0x3e, 1B */
    uint8_t field_0x3f;  /* offset 0x3f, 1B */
    uint8_t field_0x40;  /* offset 0x40, 1B */
    uint8_t screen_sync_r;  /* offset 0x41, 1B */
    uint8_t screen_sync_g;  /* offset 0x42, 1B */
    uint8_t screen_sync_b;  /* offset 0x43, 1B */
    uint8_t column_color_state[84];  /* offset 0x44, 84B */
    uint8_t breathe_states[1215];  /* offset 0x98, 1215B */
    uint8_t per_led_brightness[82];  /* offset 0x557, 82B */
    uint8_t per_led_phase[82];  /* offset 0x5a9, 82B */
    uint8_t per_led_color[328];  /* offset 0x5fb, 328B */
    uint8_t rain_row_bitmask[16];  /* offset 0x743, 16B */
    uint8_t field_0x753[203];  /* offset 0x753, 203B */
    uint8_t ripple_state_0[128];  /* offset 0x81e, 128B */
    uint8_t ripple_state_1[128];  /* offset 0x89e, 128B */
    uint8_t ripple_state_2[128];  /* offset 0x91e, 128B */
    uint8_t ripple_state_3[128];  /* offset 0x99e, 128B */
    uint8_t ripple_state_4[128];  /* offset 0xa1e, 128B */
    uint8_t ripple_state_5[128];  /* offset 0xa9e, 128B */
    uint8_t _gap_b1e[12];  /* offset 0xb1e, 12B */
    uint8_t wireless_rx_pending;  /* offset 0xb2a, 1B */
    uint8_t _pad_b2b;  /* offset 0xb2b, 1B */
    uint32_t led_status_bitmask;  /* offset 0xb2c, 4B */
} rgb_anim_state_t;  /* 2864 bytes */
_Static_assert(sizeof(rgb_anim_state_t) == 2864, "rgb_anim_state_t size mismatch");

typedef struct {
    uint8_t bLength;  /* offset 0x0, 1B */
    uint8_t bDescriptorType;  /* offset 0x1, 1B */
    uint16_t wTotalLength;  /* offset 0x2, 2B */
    uint8_t bNumInterfaces;  /* offset 0x4, 1B */
    uint8_t bConfigurationValue;  /* offset 0x5, 1B */
    uint8_t iConfiguration;  /* offset 0x6, 1B */
    uint8_t bmAttributes;  /* offset 0x7, 1B */
    uint8_t bMaxPower;  /* offset 0x8, 1B */
    uint8_t _pad_0x09[1];  /* offset 0x9, 1B */
} usb_configuration_desc_type;  /* 10 bytes */
_Static_assert(sizeof(usb_configuration_desc_type) == 10, "usb_configuration_desc_type size mismatch");

typedef struct {
    uint8_t speed;  /* offset 0x0, 1B */
    uint8_t dma_en;  /* offset 0x1, 1B */
    uint8_t hc_num;  /* offset 0x2, 1B */
    uint8_t ept_num;  /* offset 0x3, 1B */
    uint16_t max_size;  /* offset 0x4, 2B */
    uint16_t fifo_size;  /* offset 0x6, 2B */
    uint8_t phy_itface;  /* offset 0x8, 1B */
    uint8_t core_id;  /* offset 0x9, 1B */
    uint8_t low_power;  /* offset 0xa, 1B */
    uint8_t sof_out;  /* offset 0xb, 1B */
    uint8_t usb_id;  /* offset 0xc, 1B */
    uint8_t vbusig;  /* offset 0xd, 1B */
} usb_core_cfg;  /* 14 bytes */
_Static_assert(sizeof(usb_core_cfg) == 14, "usb_core_cfg size mismatch");

typedef struct {
    uint8_t bLength;  /* offset 0x0, 1B */
    uint8_t bDescriptorType;  /* offset 0x1, 1B */
    uint16_t bcdUSB;  /* offset 0x2, 2B */
    uint8_t bDeviceClass;  /* offset 0x4, 1B */
    uint8_t bDeviceSubClass;  /* offset 0x5, 1B */
    uint8_t bDeviceProtocol;  /* offset 0x6, 1B */
    uint8_t bMaxPacketSize0;  /* offset 0x7, 1B */
    uint16_t idVendor;  /* offset 0x8, 2B */
    uint16_t idProduct;  /* offset 0xa, 2B */
    uint16_t bcdDevice;  /* offset 0xc, 2B */
    uint8_t iManufacturer;  /* offset 0xe, 1B */
    uint8_t iProduct;  /* offset 0xf, 1B */
    uint8_t iSerialNumber;  /* offset 0x10, 1B */
    uint8_t bNumConfigurations;  /* offset 0x11, 1B */
} usb_device_desc_type;  /* 18 bytes */
_Static_assert(sizeof(usb_device_desc_type) == 18, "usb_device_desc_type size mismatch");

typedef struct {
    uint8_t bLength;  /* offset 0x0, 1B */
    uint8_t bDescriptorType;  /* offset 0x1, 1B */
    uint8_t bEndpointAddress;  /* offset 0x2, 1B */
    uint8_t bmAttributes;  /* offset 0x3, 1B */
    uint16_t wMaxPacketSize;  /* offset 0x4, 2B */
    uint8_t bInterval;  /* offset 0x6, 1B */
    uint8_t _pad_0x07[1];  /* offset 0x7, 1B */
} usb_endpoint_desc_type;  /* 8 bytes */
_Static_assert(sizeof(usb_endpoint_desc_type) == 8, "usb_endpoint_desc_type size mismatch");

typedef struct {
    uint8_t eptn;  /* offset 0x0, 1B */
    uint8_t ept_address;  /* offset 0x1, 1B */
    uint8_t inout;  /* offset 0x2, 1B */
    uint8_t trans_type;  /* offset 0x3, 1B */
    uint16_t tx_addr;  /* offset 0x4, 2B */
    uint16_t rx_addr;  /* offset 0x6, 2B */
    uint32_t maxpacket;  /* offset 0x8, 4B */
    uint8_t is_double_buffer;  /* offset 0xc, 1B */
    uint8_t stall;  /* offset 0xd, 1B */
    uint32_t status;  /* offset 0x10, 4B */
    uint8_t * trans_buf;  /* offset 0x14, 4B */
    uint32_t total_len;  /* offset 0x18, 4B */
    uint32_t trans_len;  /* offset 0x1c, 4B */
    uint32_t last_len;  /* offset 0x20, 4B */
    uint32_t rem0_len;  /* offset 0x24, 4B */
    uint32_t ept0_slen;  /* offset 0x28, 4B */
} usb_ept_info;  /* 44 bytes */
_Static_assert(sizeof(usb_ept_info) == 44, "usb_ept_info size mismatch");

typedef struct {
    uint8_t ch_num;  /* offset 0x0, 1B */
    uint8_t address;  /* offset 0x1, 1B */
    uint8_t dir;  /* offset 0x2, 1B */
    uint8_t ept_num;  /* offset 0x3, 1B */
    uint8_t ept_type;  /* offset 0x4, 1B */
    uint32_t maxpacket;  /* offset 0x8, 4B */
    uint8_t data_pid;  /* offset 0xc, 1B */
    uint8_t speed;  /* offset 0xd, 1B */
    uint8_t stall;  /* offset 0xe, 1B */
    uint32_t status;  /* offset 0x10, 4B */
    uint32_t state;  /* offset 0x14, 4B */
    uint32_t urb_sts;  /* offset 0x18, 4B */
    uint8_t do_ping;  /* offset 0x1c, 1B */
    uint8_t toggle_in;  /* offset 0x1d, 1B */
    uint8_t toggle_out;  /* offset 0x1e, 1B */
    uint8_t * trans_buf;  /* offset 0x20, 4B */
    uint32_t trans_len;  /* offset 0x24, 4B */
    uint32_t trans_count;  /* offset 0x28, 4B */
} usb_hch_type;  /* 44 bytes */
_Static_assert(sizeof(usb_hch_type) == 44, "usb_hch_type size mismatch");

typedef struct {
    uint8_t bLength;  /* offset 0x0, 1B */
    uint8_t bDescriptorType;  /* offset 0x1, 1B */
} usb_header_desc_type;  /* 2 bytes */
_Static_assert(sizeof(usb_header_desc_type) == 2, "usb_header_desc_type size mismatch");

typedef struct {
    uint8_t bLength;  /* offset 0x0, 1B */
    uint8_t bDescriptorType;  /* offset 0x1, 1B */
    uint8_t bInterfaceNumber;  /* offset 0x2, 1B */
    uint8_t bAlternateSetting;  /* offset 0x3, 1B */
    uint8_t bNumEndpoints;  /* offset 0x4, 1B */
    uint8_t bInterfaceClass;  /* offset 0x5, 1B */
    uint8_t bInterfaceSubClass;  /* offset 0x6, 1B */
    uint8_t bInterfaceProtocol;  /* offset 0x7, 1B */
    uint8_t iInterface;  /* offset 0x8, 1B */
} usb_interface_desc_type;  /* 9 bytes */
_Static_assert(sizeof(usb_interface_desc_type) == 9, "usb_interface_desc_type size mismatch");

typedef struct {
    uint8_t bmRequestType;  /* offset 0x0, 1B */
    uint8_t bRequest;  /* offset 0x1, 1B */
    uint16_t wValue;  /* offset 0x2, 2B */
    uint16_t wIndex;  /* offset 0x4, 2B */
    uint16_t wLength;  /* offset 0x6, 2B */
} usb_setup_pkt_t;  /* 8 bytes */
_Static_assert(sizeof(usb_setup_pkt_t) == 8, "usb_setup_pkt_t size mismatch");

typedef struct {
    void * usb_reg;  /* offset 0x0, 4B */
    void * class_cb;  /* offset 0x4, 4B */
    void * desc_cb;  /* offset 0x8, 4B */
    uint8_t ep_in_raw[352];  /* offset 0xc, 352B */
    uint8_t ep_out_raw[352];  /* offset 0x16c, 352B */
    usb_setup_pkt_t setup;  /* offset 0x2cc, 8B */
    uint8_t setup_raw[8];  /* offset 0x2d4, 8B */
    uint8_t _rsvd_2dc[40];  /* offset 0x2dc, 40B */
    uint8_t ep0_state;  /* offset 0x304, 1B */
    uint8_t hs_cfg;  /* offset 0x305, 1B */
    uint16_t ctrl_len;  /* offset 0x306, 2B */
    uint8_t dev_state;  /* offset 0x308, 1B */
    uint8_t dev_state_bak;  /* offset 0x309, 1B */
    uint8_t dev_addr;  /* offset 0x30a, 1B */
    uint8_t remote_wakeup;  /* offset 0x30b, 1B */
    uint8_t _rsvd_30c[4];  /* offset 0x30c, 4B */
    uint32_t active_cfg;  /* offset 0x310, 4B */
    uint32_t dev_status;  /* offset 0x314, 4B */
    uint8_t _rsvd_318[4];  /* offset 0x318, 4B */
    uint8_t desc_index;  /* offset 0x31c, 1B */
    uint8_t _tail_pad[3];  /* offset 0x31d, 3B */
} otg_dev_handle_t;  /* 800 bytes */
_Static_assert(sizeof(otg_dev_handle_t) == 800, "otg_dev_handle_t size mismatch");

typedef struct {
    uint8_t bmRequestType;  /* offset 0x0, 1B */
    uint8_t bRequest;  /* offset 0x1, 1B */
    uint16_t wValue;  /* offset 0x2, 2B */
    uint16_t wIndex;  /* offset 0x4, 2B */
    uint16_t wLength;  /* offset 0x6, 2B */
} usb_setup_type;  /* 8 bytes */
_Static_assert(sizeof(usb_setup_type) == 8, "usb_setup_type size mismatch");

typedef struct {
    void * init_handler;  /* offset 0x0, 4B */
    void * clear_handler;  /* offset 0x4, 4B */
    void * setup_handler;  /* offset 0x8, 4B */
    void * ept0_tx_handler;  /* offset 0xc, 4B */
    void * ept0_rx_handler;  /* offset 0x10, 4B */
    void * in_handler;  /* offset 0x14, 4B */
    void * out_handler;  /* offset 0x18, 4B */
    void * sof_handler;  /* offset 0x1c, 4B */
    void * event_handler;  /* offset 0x20, 4B */
    void * pdata;  /* offset 0x24, 4B */
} usbd_class_handler;  /* 40 bytes */
_Static_assert(sizeof(usbd_class_handler) == 40, "usbd_class_handler size mismatch");

typedef struct {
    void * get_device_descriptor;  /* offset 0x0, 4B */
    void * get_device_qualifier;  /* offset 0x4, 4B */
    void * get_device_configuration;  /* offset 0x8, 4B */
    void * get_device_other_speed;  /* offset 0xc, 4B */
    void * get_device_lang_id;  /* offset 0x10, 4B */
    void * get_device_manufacturer_string;  /* offset 0x14, 4B */
    void * get_device_product_string;  /* offset 0x18, 4B */
    void * get_device_serial_string;  /* offset 0x1c, 4B */
    void * get_device_interface_string;  /* offset 0x20, 4B */
    void * get_device_config_string;  /* offset 0x24, 4B */
    void * get_hs_device_configuration;  /* offset 0x28, 4B */
} usbd_desc_handler;  /* 44 bytes */
_Static_assert(sizeof(usbd_desc_handler) == 44, "usbd_desc_handler size mismatch");

typedef struct {
    uint16_t length;  /* offset 0x0, 2B */
    uint8_t * descriptor;  /* offset 0x4, 4B */
} usbd_desc_t;  /* 8 bytes */
_Static_assert(sizeof(usbd_desc_t) == 8, "usbd_desc_t size mismatch");

typedef struct {
    uint16_t fap;  /* offset 0x0, 2B */
    uint16_t ssb;  /* offset 0x2, 2B */
    uint16_t data0;  /* offset 0x4, 2B */
    uint16_t data1;  /* offset 0x6, 2B */
    uint16_t epp0;  /* offset 0x8, 2B */
    uint16_t epp1;  /* offset 0xa, 2B */
    uint16_t epp2;  /* offset 0xc, 2B */
    uint16_t epp3;  /* offset 0xe, 2B */
} usd_type;  /* 16 bytes */
_Static_assert(sizeof(usd_type) == 16, "usd_type size mismatch");

typedef struct {
    uint8_t cmd_ready;  /* offset 0x0, 1B */
    uint8_t cmd_ack;  /* offset 0x1, 1B */
    uint8_t cmd_id;  /* offset 0x2, 1B */
    uint8_t cmd_params[7];  /* offset 0x3, 7B */
    uint8_t cmd_data[56];  /* offset 0xa, 56B */
    uint8_t staging_buffer[512];  /* offset 0x42, 512B */
    uint8_t staging_layer;  /* offset 0x242, 1B */
    uint8_t staging_page;  /* offset 0x243, 1B */
    uint8_t staging_profile;  /* offset 0x244, 1B */
    uint8_t staging_mode;  /* offset 0x245, 1B */
    uint8_t staging_row;  /* offset 0x246, 1B */
    uint8_t staging_chunk_len;  /* offset 0x247, 1B */
    uint16_t staging_count;  /* offset 0x248, 2B */
    uint8_t staging_dirty;  /* offset 0x24a, 1B */
    uint8_t staging_commit;  /* offset 0x24b, 1B */
} vendor_cmd_buf_t;  /* 588 bytes */
_Static_assert(sizeof(vendor_cmd_buf_t) == 588, "vendor_cmd_buf_t size mismatch");

/* ── Flash regions ─────────────────────────────────────────────────────── */
extern volatile uint8_t FLASH_CONFIG_HDR[];
extern volatile uint8_t FLASH_KEYMAPS[];
extern volatile uint8_t FLASH_FN_LAYERS[];
extern volatile uint8_t FLASH_MACROS[];
extern volatile uint8_t FLASH_USERPICS[];
extern volatile uint8_t FLASH_CALIB[];
extern volatile uint8_t FLASH_MAGNETISM[];

/* ── ROM data (firmware flash) ──────────────────────────────────────────── */
extern const uint8_t boot_VTABLE[];
extern const uint8_t boot_VT_Initial_SP[];
extern const uint8_t app_VTABLE[];
extern const uint8_t app_VT_Initial_SP[];
extern const uint8_t boot_VT_Reset_Handler[];
extern const uint8_t app_VT_Reset_Handler[];
extern const uint8_t boot_VT_NMI_Handler[];
extern const uint8_t app_VT_NMI_Handler[];
extern const uint8_t boot_VT_HardFault_Handler[];
extern const uint8_t app_VT_HardFault_Handler[];
extern const uint8_t boot_VT_MemManage_Handler[];
extern const uint8_t app_VT_MemManage_Handler[];
extern const uint8_t boot_VT_BusFault_Handler[];
extern const uint8_t app_VT_BusFault_Handler[];
extern const uint8_t boot_VT_UsageFault_Handler[];
extern const uint8_t app_VT_UsageFault_Handler[];
extern const uint8_t boot_VT_Reserved7[];
extern const uint8_t app_VT_Reserved7[];
extern const uint8_t boot_VT_Reserved8[];
extern const uint8_t app_VT_Reserved8[];
extern const uint8_t boot_VT_Reserved9[];
extern const uint8_t app_VT_Reserved9[];
extern const uint8_t boot_VT_Reserved10[];
extern const uint8_t app_VT_Reserved10[];
extern const uint8_t boot_VT_SVCall_Handler[];
extern const uint8_t app_VT_SVCall_Handler[];
extern const uint8_t boot_VT_DebugMon_Handler[];
extern const uint8_t app_VT_DebugMon_Handler[];
extern const uint8_t boot_VT_Reserved13[];
extern const uint8_t app_VT_Reserved13[];
extern const uint8_t boot_VT_PendSV_Handler[];
extern const uint8_t app_VT_PendSV_Handler[];
extern const uint8_t boot_VT_SysTick_Handler[];
extern const uint8_t app_VT_SysTick_Handler[];
extern const uint8_t boot_VT_IRQ_16[];
extern const uint8_t app_VT_IRQ_16[];
extern const uint8_t boot_VT_IRQ_17[];
extern const uint8_t app_VT_IRQ_17[];
extern const uint8_t boot_VT_IRQ_18[];
extern const uint8_t app_VT_IRQ_18[];
extern const uint8_t boot_VT_IRQ_19[];
extern const uint8_t app_VT_IRQ_19[];
extern const uint8_t boot_VT_IRQ_20[];
extern const uint8_t app_VT_IRQ_20[];
extern const uint8_t boot_VT_IRQ_21[];
extern const uint8_t app_VT_IRQ_21[];
extern const uint8_t boot_VT_IRQ_22[];
extern const uint8_t app_VT_IRQ_22[];
extern const uint8_t boot_VT_IRQ_23[];
extern const uint8_t app_VT_IRQ_23[];
extern const uint8_t boot_VT_IRQ_24[];
extern const uint8_t app_VT_IRQ_24[];
extern const uint8_t boot_VT_IRQ_25[];
extern const uint8_t app_VT_IRQ_25[];
extern const uint8_t boot_VT_IRQ_26[];
extern const uint8_t app_VT_IRQ_26[];
extern const uint8_t boot_VT_IRQ_27[];
extern const uint8_t app_VT_IRQ_27[];
extern const uint8_t boot_VT_IRQ_28[];
extern const uint8_t app_VT_IRQ_28[];
extern const uint8_t boot_VT_IRQ_29[];
extern const uint8_t app_VT_IRQ_29[];
extern const uint8_t boot_VT_IRQ_30[];
extern const uint8_t app_VT_IRQ_30[];
extern const uint8_t boot_VT_IRQ_31[];
extern const uint8_t app_VT_IRQ_31[];
extern const uint8_t boot_VT_IRQ_32[];
extern const uint8_t app_VT_IRQ_32[];
extern const uint8_t boot_VT_IRQ_33[];
extern const uint8_t app_VT_IRQ_33[];
extern const uint8_t boot_VT_IRQ_34[];
extern const uint8_t app_VT_IRQ_34[];
extern const uint8_t boot_VT_IRQ_35[];
extern const uint8_t app_VT_IRQ_35[];
extern const uint8_t boot_VT_IRQ_36[];
extern const uint8_t app_VT_IRQ_36[];
extern const uint8_t boot_VT_IRQ_37[];
extern const uint8_t app_VT_IRQ_37[];
extern const uint8_t boot_VT_IRQ_38[];
extern const uint8_t app_VT_IRQ_38[];
extern const uint8_t boot_VT_IRQ_39[];
extern const uint8_t app_VT_IRQ_39[];
extern const uint8_t boot_VT_IRQ_40[];
extern const uint8_t app_VT_IRQ_40[];
extern const uint8_t boot_VT_IRQ_41[];
extern const uint8_t app_VT_IRQ_41[];
extern const uint8_t boot_VT_IRQ_42[];
extern const uint8_t app_VT_IRQ_42[];
extern const uint8_t boot_VT_IRQ_43[];
extern const uint8_t app_VT_IRQ_43[];
extern const uint8_t boot_VT_IRQ_44[];
extern const uint8_t app_VT_IRQ_44[];
extern const uint8_t boot_VT_IRQ_45[];
extern const uint8_t app_VT_IRQ_45[];
extern const uint8_t boot_VT_IRQ_46[];
extern const uint8_t app_VT_IRQ_46[];
extern const uint8_t boot_VT_IRQ_47[];
extern const uint8_t app_VT_IRQ_47[];
extern const uint8_t boot_VT_IRQ_48[];
extern const uint8_t app_VT_IRQ_48[];
extern const uint8_t boot_VT_IRQ_49[];
extern const uint8_t app_VT_IRQ_49[];
extern const uint8_t boot_VT_IRQ_50[];
extern const uint8_t app_VT_IRQ_50[];
extern const uint8_t boot_VT_IRQ_51[];
extern const uint8_t app_VT_IRQ_51[];
extern const uint8_t boot_VT_IRQ_52[];
extern const uint8_t app_VT_IRQ_52[];
extern const uint8_t boot_VT_IRQ_53[];
extern const uint8_t app_VT_IRQ_53[];
extern const uint8_t boot_VT_IRQ_54[];
extern const uint8_t app_VT_IRQ_54[];
extern const uint8_t boot_VT_IRQ_55[];
extern const uint8_t app_VT_IRQ_55[];
extern const uint8_t boot_VT_IRQ_56[];
extern const uint8_t app_VT_IRQ_56[];
extern const uint8_t boot_VT_IRQ_57[];
extern const uint8_t app_VT_IRQ_57[];
extern const uint8_t boot_VT_IRQ_58[];
extern const uint8_t app_VT_IRQ_58[];
extern const uint8_t boot_VT_IRQ_59[];
extern const uint8_t app_VT_IRQ_59[];
extern const uint8_t boot_VT_IRQ_60[];
extern const uint8_t app_VT_IRQ_60[];
extern const uint8_t boot_VT_IRQ_61[];
extern const uint8_t app_VT_IRQ_61[];
extern const uint8_t boot_VT_IRQ_62[];
extern const uint8_t app_VT_IRQ_62[];
extern const uint8_t boot_VT_IRQ_63[];
extern const uint8_t app_VT_IRQ_63[];
extern const uint8_t ptr_mag_base_4410[];
extern const uint8_t ptr_mag_base_1410[];
extern const uint8_t get_mag_cmd_buf[];
extern const uint8_t get_mag_per_key_state[];
extern const uint8_t set_mag_cmd_buf[];
extern const uint8_t set_mag_per_key_state[];
extern const uint8_t set_mag_dks_dirty_ptr[];
extern const uint8_t set_mag_config_dirty_ptr[];
extern const uint8_t ptr_cmd_buf_sm[];
extern const uint8_t ptr_macro_staging[];
extern const uint8_t ptr_dirty_flags_sm[];
extern const uint8_t ptr_mag_base_5410[];
extern const uint8_t ptr_g_mag_engine_state[];
extern const uint8_t ptr_dirty_flags_csa[];
extern const uint8_t ptr_bt_state_csa[];
extern const uint8_t ptr_cmd_buf_csa[];
extern const uint8_t ptr_FLASH_KEYMAPS[];
extern const uint8_t ptr_keymap_buf[];
extern const uint8_t ptr_config_flags2[];
extern const uint8_t ptr_dirty_flags_fl[];
extern const uint8_t ptr_bt_state_fl[];
extern const uint8_t ptr_save_active_fl[];
extern const uint8_t ptr_cmd_buf_fl[];
extern const uint8_t ptr_FLASH_FN_LAYERS[];
extern const uint8_t ptr_fn_layer_buf0[];
extern const uint8_t ptr_fn_layer_buf1[];
extern const uint8_t ptr_profile_config[];
extern const connection_config_t * ptr_g_fw_config_5[];
extern const uint8_t ptr_g_rgb_anim_state_13[];
extern const connection_config_t * ptr_g_fw_config[];
extern const uint8_t ptr_g_rgb_anim_state_5[];
extern const uint8_t ptr_flash_response_curves_2[];
extern const uint8_t DIV6_MAGIC[];
extern const uint8_t * ptr_g_userpic_buf[];
extern const uint8_t ptr_g_rgb_anim_state_15[];
extern const uint8_t ptr_g_rgb_anim_state_6[];
extern const uint8_t firework_group_tbl[];
extern const uint8_t firework_pattern_tbl[];
extern const uint8_t ptr_flash_response_curves_3[];
extern const connection_config_t * firework_fw_config[];
extern const uint8_t firework_DIV6_MAGIC[];
extern const uint8_t twinkle_DIV255_MAGIC[];
extern const uint8_t twinkle_DIV100_MAGIC[];
extern const uint8_t twinkle_DIV10_MAGIC[];
extern const uint8_t twinkle_palette_tbl[];
extern const connection_config_t * twinkle_fw_config[];
extern const uint8_t ptr_g_rgb_anim_state_2[];
extern const uint8_t ptr_g_rgb_anim_state_3[];
extern const uint8_t cross_scan_palette_tbl[];
extern const uint8_t cross_scan_pattern_tbl[];
extern const connection_config_t * cross_scan_fw_config[];
extern const uint8_t reactive_ripple_anim_data[];
extern const uint8_t helix_led_pos_tbl[];
extern const uint8_t helix_palette_tbl[];
extern const connection_config_t * helix_fw_config[];
extern const uint8_t ptr_g_rgb_anim_state_7[];
extern const uint8_t raindrop_DIV6_MAGIC[];
extern const uint8_t raindrop_led_pos_tbl[];
extern const connection_config_t * raindrop_fw_config[];
extern const uint8_t audio_viz_frame_buf[];
extern const uint8_t audio_viz_freq_data[];
extern const uint8_t audio_viz_led_pos_tbl[];
extern const uint8_t audio_viz_column_map[];
extern const connection_config_t * audio_viz_fw_config[];
extern const uint8_t audio_viz_led_pos_tbl_2[];
extern const uint8_t ptr_flash_led_position_tbl[];
extern const uint8_t audio_viz_peak_data[];
extern const uint8_t array_lookup_fragment[];
extern const uint8_t Default_IRQHandler[];
extern const uint8_t ptr_g_rgb_anim_state_14[];
extern const uint8_t flag_led_pos_tbl[];
extern const connection_config_t * flag_fw_config[];
extern const uint8_t ptr_g_rgb_anim_state_16[];
extern const uint8_t ptr_g_rgb_anim_state_8[];
extern const uint8_t ptr_flash_response_curves_4[];
extern const uint8_t ptr_g_rgb_anim_state_9[];
extern const uint8_t ptr_g_rgb_anim_state[];
extern const uint8_t ptr_g_rgb_anim_state_17[];
extern const uint8_t ripple_auto_reactive_key_state[];
extern const uint8_t ripple_auto_anim_ripple_data[];
extern const uint8_t ptr_g_rgb_anim_state_10[];
extern const uint8_t snake_palette_tbl[];
extern const uint8_t snake_DIV6_MAGIC[];
extern const connection_config_t * snake_fw_config[];
extern const uint8_t snake_trail_tbl[];
extern const uint8_t snake_pos_tbl[];
extern const uint8_t static_palette_tbl[];
extern const connection_config_t * static_fw_config[];
extern const uint8_t rgb_led_compute_hsv[];
extern const uint8_t DMA1_Ch6_IRQHandler[];
extern const uint8_t rgb_led_update_color[];
extern const uint8_t ptr_g_rgb_anim_state_11[];
extern const uint8_t spiral_palette_tbl[];
extern const uint8_t spiral_DIV6_MAGIC[];
extern const connection_config_t * spiral_fw_config[];
extern const uint8_t userpic_ram_buf[];
extern const uint8_t userpic_flash_storage[];
extern const uint8_t userpic_palette_tbl[];
extern const uint8_t userpic_led_pos_tbl_2[];
extern const uint8_t userpic_led_pos_tbl[];
extern const uint8_t wave_vert_palette_tbl[];
extern const uint8_t wave_vert_led_pos_tbl[];
extern const connection_config_t * wave_vert_fw_config[];
extern const uint8_t wave_palette_tbl[];
extern const connection_config_t * wave_fw_config[];
extern const uint8_t wave_led_pos_tbl[];
extern const uint8_t ptr_g_kbd_state_3[];
extern const uint32_t * ptr_g_action_key_state_2[];
extern const uint32_t * ptr_g_action_key_thresholds[];
extern const uint8_t ptr_g_timer_state_2[];
extern const uint8_t * ptr_g_usb_device_handle_2[];
extern const dirty_flags_t * ptr_g_dirty_flags_2[];
extern const connection_config_t * ptr_g_fw_config_3[];
extern const uint8_t ptr_bt_flags_2[];
extern const uint8_t ptr_key_report[];
extern const uint8_t ptr_bt_state_msp[];
extern const uint8_t ptr_macro_state_msp[];
extern const uint8_t ptr_FLASH_MACROS_msp[];
extern const uint8_t ptr_g_timer_state_plus12_2[];
extern const uint8_t ptr_kbd_config_msp[];
extern const uint8_t ptr_macro_state[];
extern const uint8_t ptr_kbd_config[];
extern const uint8_t ptr_g_timer_state_plus12[];
extern const uint8_t DIV10_MAGIC[];
extern const uint8_t ptr_FLASH_MACROS_pb[];
extern const uint8_t ptr_consumer_report_2[];
extern const uint8_t ptr_dirty_flags[];
extern const uint8_t ptr_bt_state[];
extern const uint8_t ptr_save_active[];
extern const uint8_t ptr_cmd_buf[];
extern const uint8_t ptr_FLASH_MACROS[];
extern const mag_scan_state_t * flash_cal_mag_state[];
extern const mag_scan_state_t * flash_thresh_mag_state[];
extern const uint8_t ptr_g_per_key_state_7[];
extern const uint8_t * ptr_g_per_key_state_4[];
extern const mag_scan_state_t * hall_eval_mag_state[];
extern const uint8_t * ptr_g_per_key_state_5[];
extern const mag_scan_state_t * hall_eval_mag_state_2[];
extern const uint8_t * ptr_g_per_key_state_2[];
extern const mag_scan_state_t * depth_calc_mag_state[];
extern const mag_scan_state_t * depth_calc_mag_state_2[];
extern const uint8_t * ptr_g_key_pressed_bitmask_6[];
extern const uint8_t * ptr_g_per_key_state_6[];
extern const uint8_t ptr_flash_response_curves[];
extern const mag_scan_state_t * flash_fn_mag_state[];
extern const uint8_t ptr_bt_state_mag[];
extern const uint8_t ptr_save_active_mag[];
extern const uint8_t ptr_mag_dirty[];
extern const uint8_t ptr_profile_config_mag[];
extern const uint8_t ptr_FLASH_MAGNETISM[];
extern const uint8_t ptr_FLASH_MAGNETISM_p1[];
extern const uint8_t ptr_flash_config_data[];
extern const connection_config_t * ptr_g_fw_config_6[];
extern const uint8_t ptr_flash_config_data_2[];
extern const mag_scan_state_t * dks_save_mag_state[];
extern const uint8_t * ptr_g_key_pressed_bitmask_2[];
extern const uint8_t * ptr_g_per_key_state_3[];
extern const uint8_t * ptr_g_key_pressed_bitmask_4[];
extern const uint8_t * ptr_g_per_key_state[];
extern const uint8_t ptr_g_per_key_state_8[];
extern const uint8_t * ptr_g_key_pressed_bitmask_5[];
extern const uint8_t ptr_g_hid_reports_3[];
extern const uint8_t * ptr_g_key_pressed_bitmask_3[];
extern const uint8_t DIV10_MAGIC[];
extern const uint8_t * ptr_g_key_pressed_bitmask[];
extern const uint8_t DIV10_MAGIC_2[];
extern const uint8_t ptr_g_key_pressed_bitmask_7[];
extern const rf_packet_buf_t * ptr_g_rf_packet_buf_3[];
extern const uint8_t ptr_g_timer_state_3[];
extern const uint8_t ptr_GPIOC_BASE_4[];
extern const uint8_t ptr_g_hid_reports[];
extern const uint8_t ptr_hid_6kro_plus3[];
extern const uint8_t ptr_consumer_report[];
extern const uint8_t ptr_g_conn_state_buf_2[];
extern const uint8_t ptr_dirty_flags_cl[];
extern const connection_config_t * ptr_g_fw_config_7[];
extern const connection_config_t * ptr_profile_config_cl[];
extern const uint8_t ptr_FLASH_CONFIG_HDR[];
extern const uint8_t ptr_FLASH_KEYMAPS_cl[];
extern const uint8_t ptr_FLASH_USERPICS_cl[];
extern const uint8_t ptr_brightness_table[];
extern const uint8_t ptr_led_anim_state_cl[];
extern const uint8_t ptr_gpio_periph[];
extern const uint8_t ptr_dongle_mouse_buf[];
extern const uint8_t ptr_pending_bitmap[];
extern const uint8_t ptr_dongle_kbd_buf[];
extern const uint8_t ptr_bt_flags[];
extern const uint8_t ptr_dongle_consumer_buf[];
extern const uint8_t ptr_macro_state_area[];
extern const uint8_t ptr_hid_nkro_report_2[];
extern const uint8_t ptr_hid_6kro_plus3_3[];
extern const uint8_t ptr_consumer_report_4[];
extern const connection_config_t * ptr_g_fw_config_2[];
extern const connection_config_t * ptr_profile_config_fr[];
extern const uint8_t ptr_FLASH_CONFIG_HDR_fr[];
extern const uint8_t ptr_FLASH_KEYMAPS_fr[];
extern const uint8_t ptr_FLASH_KEYMAPS_p1[];
extern const uint8_t ptr_FLASH_KEYMAPS_p2[];
extern const uint8_t ptr_FLASH_KEYMAPS_p3[];
extern const uint8_t ptr_default_keymaps[];
extern const uint8_t ptr_FLASH_FN_LAYERS_fr[];
extern const uint8_t ptr_FLASH_FN_LAYERS_p1[];
extern const uint8_t ptr_default_fn_layer0[];
extern const uint8_t ptr_default_fn_layer1[];
extern const uint8_t ptr_FLASH_MACROS_fr[];
extern const uint8_t ptr_default_macro_tpl[];
extern const uint8_t ptr_FLASH_USERPICS_fr[];
extern const uint8_t ptr_FLASH_USERPICS_p1[];
extern const uint8_t ptr_default_userpic[];
extern const uint8_t ptr_default_config[];
extern const uint8_t * depth_mon_usb_handle[];
extern const uint8_t depth_mon_kbd_state[];
extern const uint8_t * depth_mon_report_buf[];
extern const uint8_t * depth_mon_report_flags[];
extern const uint8_t ptr_bt_flags[];
extern const uint32_t * ptr_g_action_key_state[];
extern const connection_config_t * ptr_g_fw_config[];
extern const dirty_flags_t * ptr_g_dirty_flags[];
extern const uint8_t ptr_g_kbd_state_4[];
extern const uint8_t lp_hid_reports[];
extern const uint8_t * lp_ep1_tx_ready[];
extern const uint8_t ptr_g_vendor_cmd_buffer[];
extern const uint8_t ptr_cmd_buf_vcd[];
extern const uint8_t ptr_nkro_report[];
extern const uint8_t ptr_led_anim_state_vcd[];
extern const uint8_t ptr_kbd_config_vcd[];
extern const connection_config_t * PTR_g_conn_config[];
extern const connection_config_t * ptr_profile_config_vcd[];
extern const uint8_t ptr_dirty_flags_vcd[];
extern const uint8_t vcd_key_engine_base[];
extern const uint8_t ptr_unk_20002410[];
extern const uint8_t ptr_mag_dirty_vcd[];
extern const uint8_t ptr_g_fw_config_plus22[];
extern const uint8_t ptr_profile_0x22[];
extern const uint8_t ptr_FLASH_CONFIG_HDR_vcd[];
extern const uint8_t ptr_FLASH_MAGNETISM_vcd[];
extern const uint8_t lp_flash_mag_page2[];
extern const uint8_t lp_flash_mag_page3[];
extern const uint8_t lp_flash_mag_page4[];
extern const uint8_t lp_flash_mag_page5[];
extern const uint8_t lp_flash_mag_page6[];
extern const uint8_t lp_flash_mag_page7[];
extern const uint8_t lp_flash_mag_page8[];
extern const uint8_t lp_bl_mailbox_addr[];
extern const uint8_t lp_mailbox_readback[];
extern const uint8_t lp_bl_magic[];
extern const mag_scan_state_t * vcd_cal_done_flag_ptr[];
extern const uint8_t vcd_cal_count_buf[];
extern const uint8_t vcd_cal_accum_buf[];
extern const uint8_t vcd_auto_cal_done_flag_ptr[];
extern const uint8_t vcd_auto_cal_ref_buf[];
extern const uint8_t ptr_dirty_flags_up[];
extern const uint8_t ptr_bt_state_up[];
extern const uint8_t ptr_save_active_up[];
extern const uint8_t ptr_cmd_buf_up[];
extern const uint8_t ptr_FLASH_USERPICS[];
extern const uint8_t ptr_userpic_buf[];
extern const uint8_t ptr_led_anim_state[];
extern const connection_config_t * ptr_g_fw_config_8[];
extern const uint8_t lp_dma1_ch_reg[];
extern const uint8_t lp_dma1_base[];
extern const uint8_t lp_adc1_ordt[];
extern const uint8_t lp_mag_base_4410[];
extern const uint8_t lp_tmr3_base[];
extern const uint8_t lp_crm_clk_gpioa[];
extern const uint8_t lp_crm_clk_gpiob[];
extern const uint8_t lp_crm_clk_gpioc[];
extern const uint8_t lp_crm_clk_gpiod[];
extern const uint8_t lp_gpiob_base_2[];
extern const uint8_t lp_gpioc_base_2[];
extern const uint8_t lp_gpiod_base[];
extern const uint8_t lp_gpiof_base[];
extern const uint8_t lp_gpioa_base[];
extern const uint8_t ptr_g_kbd_state[];
extern const uint8_t ptr_GPIOC_BASE_2[];
extern const uint8_t ptr_usb_state_area[];
extern const uint8_t ptr_GPIOB_BASE_2[];
extern const uint8_t ptr_g_rgb_anim_state_12[];
extern const uint8_t lp_class_report_buf[];
extern const uint8_t lp_if0_report_desc[];
extern const uint8_t lp_if1_report_desc[];
extern const uint8_t lp_if2_report_desc[];
extern const uint8_t lp_if0_hid_desc[];
extern const uint8_t lp_if1_hid_desc[];
extern const uint8_t lp_if2_hid_desc[];
extern const uint8_t lp_crm_misc2[];
extern const uint8_t lp_crm_clk_otghs[];
extern const uint8_t lp_crm_pll_stable[];
extern const uint8_t ptr_mag_base_1410_2[];
extern const uint8_t ptr_mag_base_5410_2[];
extern const uint8_t lp_kbd_state[];
extern const uint8_t lp_usb_device[];
extern const uint8_t lp_timer_state[];
extern const uint8_t lp_fw_config[];
extern const uint8_t * ptr_g_led_frame_buf[];
extern const uint8_t lp_led_dma_buf[];
extern const connection_config_t * PTR_g_conn_config_2[];
extern const uint8_t PTR_g_kbd_state_3[];
extern const uint8_t ptr_I2C1_BASE[];
extern const uint8_t PTR_g_kbd_state[];
extern const uint8_t lp_mag_base_3410[];
extern const uint8_t lp_battery_adc_avg[];
extern const uint8_t lp_mag_base_5410[];
extern const uint8_t lp_div_magic_batt[];
extern const uint8_t lp_gpioc_base[];
extern const uint8_t PTR_g_dongle_flags[];
extern const uint8_t lp_gpiob_base[];
extern const uint8_t ptr_g_kbd_state_2[];
extern const uint8_t ptr_g_timer_state[];
extern const uint8_t ptr_GPIOB_BASE_3[];
extern const uint8_t ptr_GPIOC_BASE_3[];
extern const connection_config_t * ptr_g_fw_config_4[];
extern const uint8_t DIV15_MAGIC[];
extern const rf_packet_buf_t * ptr_g_rf_packet_buf_2[];
extern const uint8_t * ptr_g_usb_device_handle[];
extern const uint8_t * ptr_g_conn_state_buf[];
extern const uint8_t ptr_g_hid_reports_2[];
extern const uint8_t ptr_g_rgb_anim_state_4[];
extern const uint8_t PTR_g_kbd_state_2[];
extern const uint8_t ptr_I2C1_BASE_2[];
extern const rf_packet_buf_t * ptr_g_rf_packet_buf[];
extern const uint8_t PTR_g_dongle_flags_2[];
extern const uint8_t lp_pending_reports_bitmap[];
extern const uint8_t lp_dongle_kbd_buf[];
extern const uint8_t lp_dongle_nkro_buf[];
extern const uint8_t lp_dongle_mouse_buf[];
extern const uint8_t lp_dongle_consumer_buf[];
extern const uint8_t lp_dongle_extra_buf[];
extern const uint8_t lp_dongle_dial_buf[];
extern const uint8_t lp_flash_ctrl_base[];
extern const uint8_t lp_crm_clk_otghs_2[];
extern const uint8_t lp_pwc_base[];
extern const uint8_t lp_hick_8mhz[];
extern const uint8_t lp_crm_ctrl[];
extern const uint8_t lp_system_clk_freq[];
extern const uint8_t lp_hext_12mhz[];
extern const uint8_t lp_crm_cfg[];
extern const uint8_t lp_ahb_div_shift_tbl[];
extern const uint8_t lp_crm_pll_cfg[];
extern const uint8_t lp_const_48mhz[];
extern const uint8_t lp_crm_cfg2[];
extern const uint8_t lp_pll_div_tbl[];
extern const uint8_t ptr_GPIOA_BASE[];
extern const uint8_t lp_fw_config_2[];
extern const uint8_t lp_kbd_state_2[];
extern const uint8_t ptr_GPIOC_BASE[];
extern const uint8_t ptr_GPIOB_BASE[];
extern const uint8_t lp_crm_clk_tmr1[];
extern const uint8_t CONST_00300016[];
extern const uint8_t * ptr_g_led_dma_buf[];
extern const uint8_t lp_led_frame_buf[];
extern const uint8_t lp_dma1_base_2[];
extern const uint8_t lp_dma1_mux[];
extern const uint8_t lp_spi2_base[];
extern const uint8_t lp_dma2_ch_cfg[];
extern const uint8_t lp_spi3_base[];
extern const uint8_t lp_encoder_state[];
extern const encoder_state_t * enc_init_encoder_state[];
extern const encoder_state_t * enc_tick_encoder_state[];
extern const uint8_t enc_kbd_state[];
extern const encoder_state_t * enc_encoder_state[];
extern const connection_config_t * enc_conn_config[];
extern const uint8_t * enc_keymap_table[];
extern const uint8_t * enc_gpioc_base[];
extern const uint8_t * enc_key_bitmask[];
extern const uint8_t DEFAULT_CONFIG[];
extern const uint8_t DEFAULT_KEYMAPS[];
extern const uint8_t DEFAULT_FN_LAYER0[];
extern const uint8_t DEFAULT_FN_LAYER1[];
extern const uint8_t DEFAULT_MACRO_TPL[];
extern const uint8_t DEFAULT_USERPIC[];
extern const uint8_t static_led_pos_tbl[];

/* ── RAM globals ───────────────────────────────────────────────────────── */
extern volatile uint8_t g_systick_saved_ctrl[];
extern volatile mag_scan_state_t g_mag_scan_state[];
extern volatile uint8_t g_auto_cal_done_flag[];
extern volatile uint8_t g_mag_config_flash_dirty[];
extern volatile uint8_t g_mag_dks_flash_dirty[];
extern volatile uint8_t g_battery_avg_buf[];
extern volatile uint8_t g_hid_reports[];
extern volatile uint8_t g_dongle_hid_bitmap[];
extern volatile uint8_t g_hid_report_pending_flags[];
extern volatile uint8_t g_bt_state[];
extern volatile hid_6kro_report_t g_dongle_dial_buf[];
extern volatile uint8_t g_dongle_consumer_buf[];
extern volatile uint8_t g_debounce_counter[];
extern volatile uint8_t g_dongle_kbd_buf[];
extern volatile consumer_report_t g_dongle_mouse_buf[];
extern volatile uint8_t g_action_key_thresholds[];
extern volatile led_effect_state_t g_led_flash_all_state[];
extern volatile conn_indicator_state_t g_conn_indicator[];
extern volatile uint8_t g_save_active[];
extern volatile dirty_flags_t g_dirty_flags[];
extern volatile uint8_t g_reactive_key_state[];
extern volatile usbd_class_handler g_class_handler[];
extern volatile usbd_desc_handler g_desc_handler[];
extern volatile uint8_t g_device_desc[];
extern volatile uint8_t g_cfg_desc_fs[];
extern volatile uint8_t g_cfg_desc_hs[];
extern volatile uint8_t g_cfg_desc_os[];
extern volatile uint8_t g_device_qualifier_desc[];
extern volatile uint8_t g_if0_report_desc[];
extern volatile uint8_t g_if1_report_desc[];
extern volatile uint8_t g_if2_report_desc[];
extern volatile uint8_t g_if0_hid_desc[];
extern volatile uint8_t g_if1_hid_desc[];
extern volatile uint8_t g_if2_hid_desc[];
extern volatile uint8_t g_lang_id_desc[];
extern volatile usbd_desc_t g_desct_device[];
extern volatile usbd_desc_t g_desct_cfg_fs[];
extern volatile usbd_desc_t g_desct_cfg_hs[];
extern volatile usbd_desc_t g_desct_cfg_os[];
extern volatile usbd_desc_t g_desct_lang_id[];
extern volatile usbd_desc_t g_desct_string_buf[];
extern volatile usbd_desc_t g_desct_qualifier[];
extern volatile uint8_t g_kbd_state[];
extern volatile uint8_t g_scan_counter[];
extern volatile uint8_t g_kbd_config[];
extern volatile uint8_t g_fast_tick_counter[];
extern volatile uint8_t g_connection_mode[];
extern volatile uint8_t g_macro_active[];
extern volatile uint8_t g_battery_level[];
extern volatile uint8_t g_charger_connected[];
extern volatile uint8_t g_charger_debounce_ctr[];
extern volatile uint8_t g_battery_update_ctr[];
extern volatile uint8_t g_battery_raw_level[];
extern volatile uint8_t g_animation_dirty[];
extern volatile uint8_t g_battery_indicator_active[];
extern volatile uint8_t g_low_batt_blink_state[];
extern volatile uint8_t g_battery_charging[];
extern volatile uint8_t g_low_batt_blink_ctr[];
extern volatile uint8_t g_charge_detect_flag[];
extern volatile uint8_t g_charge_detect_ctr[];
extern volatile uint8_t g_charge_status_ctr1[];
extern volatile uint8_t g_charge_status[];
extern volatile uint8_t g_charge_status_ctr2[];
extern volatile uint8_t g_led_buf_back[];
extern volatile uint8_t g_led_dma_buf[];
extern volatile uint8_t g_led_buf_front[];
extern volatile uint8_t g_led_frame_buf[];
extern volatile uint8_t g_per_key_state[];
extern volatile uint8_t g_mag_dirty[];
extern volatile uint8_t g_led_ws2812_dma_buf[];
extern volatile mag_engine_state_t g_adc_raw_buf[];
extern volatile mag_engine_state_t g_key_engine_base[];
extern volatile mag_engine_state_t g_mag_engine_state[];
extern volatile uint8_t g_depth_monitor_enable[];
extern volatile uint8_t g_calibration_enable[];
extern volatile uint8_t g_auto_cal_enable[];
extern volatile uint8_t g_mag_conn_gpio_counter[];
extern volatile uint8_t g_adc_sample_buf2[];
extern volatile uint8_t g_switch_type_table[];
extern volatile uint8_t g_switch_class_table[];
extern volatile uint8_t g_adc_scan_state[];
extern volatile uint8_t g_adc_dma_counter[];
extern volatile uint8_t g_adc_dma_flag[];
extern volatile uint8_t g_adc_accumulator[];
extern volatile uint8_t g_usb_device_config[];
extern volatile uint8_t g_adc_scan_active[];
extern volatile uint8_t g_adc_process_flag[];
extern volatile uint8_t g_usb_device[];
extern volatile uint8_t g_dongle_state[];
extern volatile uint8_t g_usb_device_handle[];
extern volatile uint8_t g_rf_packet_buf[];
extern volatile uint8_t g_rf_rx_cmd_byte[];
extern volatile uint8_t g_adc_scan_count_cfg[];
extern volatile uint8_t g_timer_state[];
extern volatile uint8_t g_dongle_report_flags[];
extern volatile uint8_t g_macro_page_buf[];
extern volatile uint8_t g_macro_playback_state[];
extern volatile uint8_t g_macro_state[];
extern volatile uint8_t g_conn_state_buf[];
extern volatile uint8_t g_conn_state_mode[];
extern volatile uint8_t g_conn_state_active[];
extern volatile uint8_t g_conn_state_flag[];
extern volatile uint8_t g_config_flags2[];
extern volatile uint8_t g_key_state[];
extern volatile uint8_t g_key_pressed_bitmask[];
extern volatile uint8_t g_action_key_state[];
extern volatile uint8_t g_encoder_state[];
extern volatile connection_config_t g_fw_config[];
extern volatile connection_config_t g_connection_config[];
extern volatile connection_config_t g_profile_config[];
extern volatile uint8_t g_keymatrix_layers[];
extern volatile uint8_t g_keymap_table[];
extern volatile uint8_t g_keymap_buf[];
extern volatile uint8_t g_fn_layer_buf0[];
extern volatile uint8_t g_fn_layer_buf1[];
extern volatile uint8_t g_userpic_buf[];
extern volatile uint8_t g_vendor_cmd_buffer[];
extern volatile uint8_t g_vendor_cmd_buf[];
extern volatile uint8_t g_cmd_buf[];
extern volatile uint8_t g_macro_staging[];
extern volatile uint8_t g_led_anim_state[];
extern volatile uint8_t g_vendor_response_ctx[];
extern volatile uint8_t g_wireless_rx_pending[];
extern volatile uint8_t g_stack_top[];

/* ── MMIO registers ────────────────────────────────────────────────────── */
extern volatile uint32_t TMR2_CTRL1_TEST;
extern volatile uint32_t TMR2_CTRL1;
extern volatile uint32_t TMR2_CTRL2;
extern volatile uint32_t TMR2_STCTRL;
extern volatile uint32_t TMR2_IDEN;
extern volatile uint32_t TMR2_ISTS;
extern volatile uint32_t TMR2_SWEVT;
extern volatile uint32_t TMR2_CM1_INPUT;
extern volatile uint32_t TMR2_CM2_INPUT;
extern volatile uint32_t TMR2_CCTRL;
extern volatile uint32_t TMR2_CVAL;
extern volatile uint32_t TMR2_DIV;
extern volatile uint32_t TMR2_PR;
extern volatile uint32_t TMR2_C1DT;
extern volatile uint32_t TMR2_C2DT;
extern volatile uint32_t TMR2_C3DT;
extern volatile uint32_t TMR2_C4DT;
extern volatile uint32_t TMR2_DMACTRL;
extern volatile uint32_t TMR2_DMADT;
extern volatile uint32_t TMR2_RMP;
extern volatile uint32_t TMR3_CTRL1;
extern volatile uint32_t TMR3_CTRL2;
extern volatile uint32_t TMR3_STCTRL;
extern volatile uint32_t TMR3_IDEN;
extern volatile uint32_t TMR3_ISTS;
extern volatile uint32_t TMR3_SWEVT;
extern volatile uint32_t TMR3_CM1_INPUT;
extern volatile uint32_t TMR3_CM2_INPUT;
extern volatile uint32_t TMR3_CCTRL;
extern volatile uint32_t TMR3_CVAL;
extern volatile uint32_t TMR3_DIV;
extern volatile uint32_t TMR3_PR;
extern volatile uint32_t TMR3_C1DT;
extern volatile uint32_t TMR3_C2DT;
extern volatile uint32_t TMR3_C3DT;
extern volatile uint32_t TMR3_C4DT;
extern volatile uint32_t TMR3_DMACTRL;
extern volatile uint32_t TMR3_DMADT;
extern volatile uint32_t TMR4_CTRL1;
extern volatile uint32_t TMR4_CTRL2;
extern volatile uint32_t TMR4_STCTRL;
extern volatile uint32_t TMR4_IDEN;
extern volatile uint32_t TMR4_ISTS;
extern volatile uint32_t TMR4_SWEVT;
extern volatile uint32_t TMR4_CM1_INPUT;
extern volatile uint32_t TMR4_CM2_INPUT;
extern volatile uint32_t TMR4_CCTRL;
extern volatile uint32_t TMR4_CVAL;
extern volatile uint32_t TMR4_DIV;
extern volatile uint32_t TMR4_PR;
extern volatile uint32_t TMR4_C1DT;
extern volatile uint32_t TMR4_C2DT;
extern volatile uint32_t TMR4_C3DT;
extern volatile uint32_t TMR4_C4DT;
extern volatile uint32_t TMR4_DMACTRL;
extern volatile uint32_t TMR4_DMADT;
extern volatile uint32_t TMR6_CTRL1;
extern volatile uint32_t TMR5_BASE;
extern volatile uint32_t TMR6_CTRL2;
extern volatile uint32_t TMR6_IDEN;
extern volatile uint32_t TMR6_ISTS;
extern volatile uint32_t TMR6_SWEVT;
extern volatile uint32_t TMR6_CVAL;
extern volatile uint32_t TMR6_DIV;
extern volatile uint32_t TMR6_PR;
extern volatile uint32_t TMR7_CTRL1;
extern volatile uint32_t TMR7_CTRL2;
extern volatile uint32_t TMR7_IDEN;
extern volatile uint32_t TMR7_ISTS;
extern volatile uint32_t TMR7_SWEVT;
extern volatile uint32_t TMR7_CVAL;
extern volatile uint32_t TMR7_DIV;
extern volatile uint32_t TMR7_PR;
extern volatile uint32_t TMR13_CTRL1;
extern volatile uint32_t TMR13_CTRL2;
extern volatile uint32_t TMR13_IDEN;
extern volatile uint32_t TMR13_ISTS;
extern volatile uint32_t TMR13_SWEVT;
extern volatile uint32_t TMR13_CM1_INPUT;
extern volatile uint32_t TMR13_CCTRL;
extern volatile uint32_t TMR13_CVAL;
extern volatile uint32_t TMR13_DIV;
extern volatile uint32_t TMR13_PR;
extern volatile uint32_t TMR13_RPR;
extern volatile uint32_t TMR13_C1DT;
extern volatile uint32_t TMR13_BRK;
extern volatile uint32_t TMR13_DMACTRL;
extern volatile uint32_t TMR13_DMADT;
extern volatile uint32_t TMR14_RMP;
extern volatile uint32_t ERTC_TIME;
extern volatile uint32_t ERTC_DATE;
extern volatile uint32_t ERTC_CTRL;
extern volatile uint32_t ERTC_STS;
extern volatile uint32_t ERTC_DIV;
extern volatile uint32_t ERTC_WAT;
extern volatile uint32_t ERTC_CCAL;
extern volatile uint32_t ERTC_WP;
extern volatile uint32_t ERTC_SBS;
extern volatile uint32_t ERTC_TADJ;
extern volatile uint32_t ERTC_TSTM;
extern volatile uint32_t ERTC_TSDT;
extern volatile uint32_t ERTC_TSSBS;
extern volatile uint32_t ERTC_SCAL;
extern volatile uint32_t ERTC_TAMP;
extern volatile uint32_t ERTC_ALASBS;
extern volatile uint32_t ERTC_ALBSBS;
extern volatile uint32_t ERTC_BPR1DT;
extern volatile uint32_t ERTC_BPR2DT;
extern volatile uint32_t ERTC_BPR3DT;
extern volatile uint32_t ERTC_BPR4DT;
extern volatile uint32_t ERTC_BPR5DT;
extern volatile uint32_t ERTC_BPR6DT;
extern volatile uint32_t ERTC_BPR7DT;
extern volatile uint32_t ERTC_BPR8DT;
extern volatile uint32_t ERTC_BPR9DT;
extern volatile uint32_t ERTC_BPR10DT;
extern volatile uint32_t ERTC_BPR11DT;
extern volatile uint32_t ERTC_BPR12DT;
extern volatile uint32_t ERTC_BPR13DT;
extern volatile uint32_t ERTC_BPR14DT;
extern volatile uint32_t ERTC_BPR15DT;
extern volatile uint32_t ERTC_BPR16DT;
extern volatile uint32_t ERTC_BPR17DT;
extern volatile uint32_t ERTC_BPR18DT;
extern volatile uint32_t ERTC_BPR19DT;
extern volatile uint32_t ERTC_BPR20DT;
extern volatile uint32_t WWDT_CTRL;
extern volatile uint32_t WWDT_CFG;
extern volatile uint32_t WWDT_STS;
extern volatile uint32_t WDT_CMD;
extern volatile uint32_t WDT_DIV;
extern volatile uint32_t WDT_RLD;
extern volatile uint32_t WDT_STS;
extern volatile uint32_t WDT_WIN;
extern volatile uint32_t SPI2_CTRL1;
extern volatile uint32_t SPI2_CTRL2;
extern volatile uint32_t SPI2_STS;
extern volatile uint32_t SPI2_DT;
extern volatile uint32_t SPI2_CPOLY;
extern volatile uint32_t SPI2_RCRC;
extern volatile uint32_t SPI2_TCRC;
extern volatile uint32_t SPI2_I2SCTRL;
extern volatile uint32_t SPI2_I2SCLK;
extern volatile uint32_t SPI3_CTRL1;
extern volatile uint32_t SPI3_CTRL2;
extern volatile uint32_t SPI3_STS;
extern volatile uint32_t SPI3_DT;
extern volatile uint32_t SPI3_CPOLY;
extern volatile uint32_t SPI3_RCRC;
extern volatile uint32_t SPI3_TCRC;
extern volatile uint32_t SPI3_I2SCTRL;
extern volatile uint32_t SPI3_I2SCLK;
extern volatile uint32_t USART2_STS;
extern volatile uint32_t USART2_DT;
extern volatile uint32_t USART2_BAUDR;
extern volatile uint32_t USART2_CTRL1;
extern volatile uint32_t USART2_CTRL2;
extern volatile uint32_t USART2_CTRL3;
extern volatile uint32_t USART2_GDIV;
extern volatile uint32_t USART2_RTOV;
extern volatile uint32_t USART2_IFC;
extern volatile uint32_t USART3_STS;
extern volatile uint32_t USART3_DT;
extern volatile uint32_t USART3_BAUDR;
extern volatile uint32_t USART3_CTRL1;
extern volatile uint32_t USART3_CTRL2;
extern volatile uint32_t USART3_CTRL3;
extern volatile uint32_t USART3_GDIV;
extern volatile uint32_t USART3_RTOV;
extern volatile uint32_t USART3_IFC;
extern volatile uint32_t USART4_STS;
extern volatile uint32_t USART4_DT;
extern volatile uint32_t USART4_BAUDR;
extern volatile uint32_t USART4_CTRL1;
extern volatile uint32_t USART4_CTRL2;
extern volatile uint32_t USART4_CTRL3;
extern volatile uint32_t USART4_GDIV;
extern volatile uint32_t USART4_RTOV;
extern volatile uint32_t USART4_IFC;
extern volatile uint32_t USART5_STS;
extern volatile uint32_t USART5_DT;
extern volatile uint32_t USART5_BAUDR;
extern volatile uint32_t USART5_CTRL1;
extern volatile uint32_t USART5_CTRL2;
extern volatile uint32_t USART5_CTRL3;
extern volatile uint32_t USART5_GDIV;
extern volatile uint32_t USART5_RTOV;
extern volatile uint32_t USART5_IFC;
extern volatile uint32_t I2C1_CTRL1;
extern volatile uint32_t I2C1_CTRL2;
extern volatile uint32_t I2C1_OADDR1;
extern volatile uint32_t I2C1_OADDR2;
extern volatile uint32_t I2C1_CLKCTRL;
extern volatile uint32_t I2C1_TIMEOUT;
extern volatile uint32_t I2C1_STS;
extern volatile uint32_t I2C1_CLR;
extern volatile uint32_t I2C1_PEC;
extern volatile uint32_t I2C1_RXDT;
extern volatile uint32_t I2C1_TXDT;
extern volatile uint32_t I2C2_CTRL1;
extern volatile uint32_t I2C2_CTRL2;
extern volatile uint32_t I2C2_OADDR1;
extern volatile uint32_t I2C2_OADDR2;
extern volatile uint32_t I2C2_CLKCTRL;
extern volatile uint32_t I2C2_TIMEOUT;
extern volatile uint32_t I2C2_STS;
extern volatile uint32_t I2C2_CLR;
extern volatile uint32_t I2C2_PEC;
extern volatile uint32_t I2C2_RXDT;
extern volatile uint32_t I2C2_TXDT;
extern volatile uint32_t I2C3_CTRL1;
extern volatile uint32_t I2C3_CTRL2;
extern volatile uint32_t I2C3_OADDR1;
extern volatile uint32_t I2C3_OADDR2;
extern volatile uint32_t I2C3_CLKCTRL;
extern volatile uint32_t I2C3_TIMEOUT;
extern volatile uint32_t I2C3_STS;
extern volatile uint32_t I2C3_CLR;
extern volatile uint32_t I2C3_PEC;
extern volatile uint32_t I2C3_RXDT;
extern volatile uint32_t I2C3_TXDT;
extern volatile uint32_t CAN1_MCTRL;
extern volatile uint32_t CAN1_MSTS;
extern volatile uint32_t CAN1_TSTS;
extern volatile uint32_t CAN1_RF0;
extern volatile uint32_t CAN1_RF1;
extern volatile uint32_t CAN1_INTEN;
extern volatile uint32_t CAN1_ESTS;
extern volatile uint32_t CAN1_BTMG;
extern volatile uint32_t CAN1_TMI0;
extern volatile uint32_t CAN1_TMC0;
extern volatile uint32_t CAN1_TMDTL0;
extern volatile uint32_t CAN1_TMDTH0;
extern volatile uint32_t CAN1_TMI1;
extern volatile uint32_t CAN1_TMC1;
extern volatile uint32_t CAN1_TMDTL1;
extern volatile uint32_t CAN1_TMDTH1;
extern volatile uint32_t CAN1_TMI2;
extern volatile uint32_t CAN1_TMC2;
extern volatile uint32_t CAN1_TMDTL2;
extern volatile uint32_t CAN1_TMDTH2;
extern volatile uint32_t CAN1_RFI0;
extern volatile uint32_t CAN1_RFC0;
extern volatile uint32_t CAN1_RFDTL0;
extern volatile uint32_t CAN1_RFDTH0;
extern volatile uint32_t CAN1_RFI1;
extern volatile uint32_t CAN1_RFC1;
extern volatile uint32_t CAN1_RFDTL1;
extern volatile uint32_t CAN1_RFDTH1;
extern volatile uint32_t CAN1_FCTRL;
extern volatile uint32_t CAN1_FMCFG;
extern volatile uint32_t CAN1_FBWCFG;
extern volatile uint32_t CAN1_FRF;
extern volatile uint32_t CAN1_FACFG;
extern volatile uint32_t CAN1_F0FB1;
extern volatile uint32_t CAN1_F0FB2;
extern volatile uint32_t CAN1_F1FB1;
extern volatile uint32_t CAN1_F1FB2;
extern volatile uint32_t CAN1_F2FB1;
extern volatile uint32_t CAN1_F2FB2;
extern volatile uint32_t CAN1_F3FB1;
extern volatile uint32_t CAN1_F3FB2;
extern volatile uint32_t CAN1_F4FB1;
extern volatile uint32_t CAN1_F4FB2;
extern volatile uint32_t CAN1_F5FB1;
extern volatile uint32_t CAN1_F5FB2;
extern volatile uint32_t CAN1_F6FB1;
extern volatile uint32_t CAN1_F6FB2;
extern volatile uint32_t CAN1_F7FB1;
extern volatile uint32_t CAN1_F7FB2;
extern volatile uint32_t CAN1_F8FB1;
extern volatile uint32_t CAN1_F8FB2;
extern volatile uint32_t CAN1_F9FB1;
extern volatile uint32_t CAN1_F9FB2;
extern volatile uint32_t CAN1_F10FB1;
extern volatile uint32_t CAN1_F10FB2;
extern volatile uint32_t CAN1_F11FB1;
extern volatile uint32_t CAN1_F11FB2;
extern volatile uint32_t CAN1_F12FB1;
extern volatile uint32_t CAN1_F12FB2;
extern volatile uint32_t CAN1_F13FB1;
extern volatile uint32_t CAN1_F13FB2;
extern volatile uint32_t PWC_CTRL;
extern volatile uint32_t PWC_CTRLSTS;
extern volatile uint32_t PWC_LDOOV;
extern volatile uint32_t UART7_STS;
extern volatile uint32_t UART7_DT;
extern volatile uint32_t UART7_BAUDR;
extern volatile uint32_t UART7_CTRL1;
extern volatile uint32_t UART7_CTRL2;
extern volatile uint32_t UART7_CTRL3;
extern volatile uint32_t UART7_GDIV;
extern volatile uint32_t UART7_RTOV;
extern volatile uint32_t UART7_IFC;
extern volatile uint32_t UART8_STS;
extern volatile uint32_t UART8_DT;
extern volatile uint32_t UART8_BAUDR;
extern volatile uint32_t UART8_CTRL1;
extern volatile uint32_t UART8_CTRL2;
extern volatile uint32_t UART8_CTRL3;
extern volatile uint32_t UART8_GDIV;
extern volatile uint32_t UART8_RTOV;
extern volatile uint32_t UART8_IFC;
extern volatile uint32_t TMR1_CTRL1;
extern volatile uint32_t TMR1_CTRL2;
extern volatile uint32_t TMR1_STCTRL;
extern volatile uint32_t TMR1_IDEN;
extern volatile uint32_t TMR1_ISTS;
extern volatile uint32_t TMR1_SWEVT;
extern volatile uint32_t TMR1_CM1_INPUT;
extern volatile uint32_t TMR1_CM2_INPUT;
extern volatile uint32_t TMR1_CCTRL;
extern volatile uint32_t TMR1_CVAL;
extern volatile uint32_t TMR1_DIV;
extern volatile uint32_t TMR1_PR;
extern volatile uint32_t TMR1_RPR;
extern volatile uint32_t TMR1_C1DT;
extern volatile uint32_t TMR1_C2DT;
extern volatile uint32_t TMR1_C3DT;
extern volatile uint32_t TMR1_C4DT;
extern volatile uint32_t TMR1_BRK;
extern volatile uint32_t TMR1_DMACTRL;
extern volatile uint32_t TMR1_DMADT;
extern volatile uint32_t TMR1_CM3_OUTPUT;
extern volatile uint32_t TMR1_C5DT;
extern volatile uint32_t USART1_STS;
extern volatile uint32_t USART1_DT;
extern volatile uint32_t USART1_BAUDR;
extern volatile uint32_t USART1_CTRL1;
extern volatile uint32_t USART1_CTRL2;
extern volatile uint32_t USART1_CTRL3;
extern volatile uint32_t USART1_GDIV;
extern volatile uint32_t USART1_RTOV;
extern volatile uint32_t USART1_IFC;
extern volatile uint32_t USART6_STS;
extern volatile uint32_t USART6_DT;
extern volatile uint32_t USART6_BAUDR;
extern volatile uint32_t USART6_CTRL1;
extern volatile uint32_t USART6_CTRL2;
extern volatile uint32_t USART6_CTRL3;
extern volatile uint32_t USART6_GDIV;
extern volatile uint32_t USART6_RTOV;
extern volatile uint32_t USART6_IFC;
extern volatile uint32_t ADC1_STS;
extern volatile uint32_t ADC1_CTRL1;
extern volatile uint32_t ADC1_CTRL2;
extern volatile uint32_t ADC1_SPT1;
extern volatile uint32_t ADC1_SPT2;
extern volatile uint32_t ADC1_PCDTO1;
extern volatile uint32_t ADC1_PCDTO2;
extern volatile uint32_t ADC1_PCDTO3;
extern volatile uint32_t ADC1_PCDTO4;
extern volatile uint32_t ADC1_VMHB;
extern volatile uint32_t ADC1_VMLB;
extern volatile uint32_t ADC1_OSQ1;
extern volatile uint32_t ADC1_OSQ2;
extern volatile uint32_t ADC1_OSQ3;
extern volatile uint32_t ADC1_PSQ;
extern volatile uint32_t ADC1_PDT1;
extern volatile uint32_t ADC1_PDT2;
extern volatile uint32_t ADC1_PDT3;
extern volatile uint32_t ADC1_PDT4;
extern volatile uint32_t ADC1_ODT;
extern volatile uint32_t ADC1_OVSP;
extern volatile uint32_t ADCCOM_CCTRL;
extern volatile uint32_t SPI1_CTRL1;
extern volatile uint32_t SPI1_CTRL2;
extern volatile uint32_t SPI1_STS;
extern volatile uint32_t SPI1_DT;
extern volatile uint32_t SPI1_CPOLY;
extern volatile uint32_t SPI1_RCRC;
extern volatile uint32_t SPI1_TCRC;
extern volatile uint32_t SPI1_I2SCTRL;
extern volatile uint32_t SPI1_I2SCLK;
extern volatile uint32_t SCFG_CFG1;
extern volatile uint32_t SCFG_CFG2;
extern volatile uint32_t SCFG_EXINTC1;
extern volatile uint32_t SCFG_EXINTC2;
extern volatile uint32_t SCFG_EXINTC3;
extern volatile uint32_t SCFG_EXINTC4;
extern volatile uint32_t SCFG_UHDRV;
extern volatile uint32_t EXINT_INTEN;
extern volatile uint32_t EXINT_EVTEN;
extern volatile uint32_t EXINT_POLCFG1;
extern volatile uint32_t EXINT_POLCFG2;
extern volatile uint32_t EXINT_SWTRG;
extern volatile uint32_t EXINT_INTSTS;
extern volatile uint32_t TMR9_CTRL1;
extern volatile uint32_t TMR9_CTRL2;
extern volatile uint32_t TMR9_STCTRL;
extern volatile uint32_t TMR9_IDEN;
extern volatile uint32_t TMR9_ISTS;
extern volatile uint32_t TMR9_SWEVT;
extern volatile uint32_t TMR9_CM1_INPUT;
extern volatile uint32_t TMR9_CCTRL;
extern volatile uint32_t TMR9_CVAL;
extern volatile uint32_t TMR9_DIV;
extern volatile uint32_t TMR9_PR;
extern volatile uint32_t TMR9_RPR;
extern volatile uint32_t TMR9_C1DT;
extern volatile uint32_t TMR9_C2DT;
extern volatile uint32_t TMR9_BRK;
extern volatile uint32_t TMR9_DMACTRL;
extern volatile uint32_t TMR9_DMADT;
extern volatile uint32_t TMR10_CTRL1;
extern volatile uint32_t TMR10_CTRL2;
extern volatile uint32_t TMR10_IDEN;
extern volatile uint32_t TMR10_ISTS;
extern volatile uint32_t TMR10_SWEVT;
extern volatile uint32_t TMR10_CM1_INPUT;
extern volatile uint32_t TMR10_CCTRL;
extern volatile uint32_t TMR10_CVAL;
extern volatile uint32_t TMR10_DIV;
extern volatile uint32_t TMR10_PR;
extern volatile uint32_t TMR10_RPR;
extern volatile uint32_t TMR10_C1DT;
extern volatile uint32_t TMR10_BRK;
extern volatile uint32_t TMR10_DMACTRL;
extern volatile uint32_t TMR10_DMADT;
extern volatile uint32_t TMR11_CTRL1;
extern volatile uint32_t TMR11_CTRL2;
extern volatile uint32_t TMR11_IDEN;
extern volatile uint32_t TMR11_ISTS;
extern volatile uint32_t TMR11_SWEVT;
extern volatile uint32_t TMR11_CM1_INPUT;
extern volatile uint32_t TMR11_CCTRL;
extern volatile uint32_t TMR11_CVAL;
extern volatile uint32_t TMR11_DIV;
extern volatile uint32_t TMR11_PR;
extern volatile uint32_t TMR11_RPR;
extern volatile uint32_t TMR11_C1DT;
extern volatile uint32_t TMR11_BRK;
extern volatile uint32_t TMR11_DMACTRL;
extern volatile uint32_t TMR11_DMADT;
extern volatile uint32_t I2SF5_CTRL2;
extern volatile uint32_t I2SF5_STS;
extern volatile uint32_t I2SF5_DT;
extern volatile uint32_t I2SF5_I2SCTRL;
extern volatile uint32_t I2SF5_I2SCLK;
extern volatile uint32_t I2SF5_MISC1;
extern volatile uint32_t ACC_STS;
extern volatile uint32_t ACC_CTRL1;
extern volatile uint32_t ACC_CTRL2;
extern volatile uint32_t ACC_C1;
extern volatile uint32_t ACC_C2;
extern volatile uint32_t ACC_C3;
extern volatile uint32_t GPIOA_CFGR;
extern volatile uint32_t GPIOA_OMODE;
extern volatile uint32_t GPIOA_ODRVR;
extern volatile uint32_t GPIOA_PULL;
extern volatile uint32_t GPIOA_IDT;
extern volatile uint32_t GPIOA_ODT;
extern volatile uint32_t GPIOA_SCR;
extern volatile uint32_t GPIOA_WPR;
extern volatile uint32_t GPIOA_MUXL;
extern volatile uint32_t GPIOA_MUXH;
extern volatile uint32_t GPIOA_CLR;
extern volatile uint32_t GPIOA_TOGR;
extern volatile uint32_t GPIOA_HDRV;
extern volatile uint32_t GPIOB_CFGR;
extern volatile uint32_t GPIOB_OMODE;
extern volatile uint32_t GPIOB_ODRVR;
extern volatile uint32_t GPIOB_PULL;
extern volatile uint32_t GPIOB_IDT;
extern volatile uint32_t GPIOB_ODT;
extern volatile uint32_t GPIOB_SCR;
extern volatile uint32_t GPIOB_WPR;
extern volatile uint32_t GPIOB_MUXL;
extern volatile uint32_t GPIOB_MUXH;
extern volatile uint32_t GPIOB_CLR;
extern volatile uint32_t GPIOB_TOGR;
extern volatile uint32_t GPIOB_HDRV;
extern volatile uint32_t GPIOC_CFGR;
extern volatile uint32_t GPIOC_OMODE;
extern volatile uint32_t GPIOC_ODRVR;
extern volatile uint32_t GPIOC_PULL;
extern volatile uint32_t GPIOC_IDT;
extern volatile uint32_t GPIOC_ODT;
extern volatile uint32_t GPIOC_SCR;
extern volatile uint32_t GPIOC_WPR;
extern volatile uint32_t GPIOC_MUXL;
extern volatile uint32_t GPIOC_MUXH;
extern volatile uint32_t GPIOC_CLR;
extern volatile uint32_t GPIOC_TOGR;
extern volatile uint32_t GPIOC_HDRV;
extern volatile uint32_t GPIOD_CFGR;
extern volatile uint32_t GPIOD_OMODE;
extern volatile uint32_t GPIOD_ODRVR;
extern volatile uint32_t GPIOD_PULL;
extern volatile uint32_t GPIOD_IDT;
extern volatile uint32_t GPIOD_ODT;
extern volatile uint32_t GPIOD_SCR;
extern volatile uint32_t GPIOD_WPR;
extern volatile uint32_t GPIOD_MUXL;
extern volatile uint32_t GPIOD_MUXH;
extern volatile uint32_t GPIOD_CLR;
extern volatile uint32_t GPIOD_TOGR;
extern volatile uint32_t GPIOD_HDRV;
extern volatile uint32_t GPIOF_CFGR;
extern volatile uint32_t GPIOF_OMODE;
extern volatile uint32_t GPIOF_ODRVR;
extern volatile uint32_t GPIOF_PULL;
extern volatile uint32_t GPIOF_IDT;
extern volatile uint32_t GPIOF_ODT;
extern volatile uint32_t GPIOF_SCR;
extern volatile uint32_t GPIOF_WPR;
extern volatile uint32_t GPIOF_MUXL;
extern volatile uint32_t GPIOF_MUXH;
extern volatile uint32_t GPIOF_CLR;
extern volatile uint32_t GPIOF_TOGR;
extern volatile uint32_t GPIOF_HDRV;
extern volatile uint32_t CRC_DT;
extern volatile uint32_t CRC_CDT;
extern volatile uint32_t CRC_CTRL;
extern volatile uint32_t CRC_IDT;
extern volatile uint32_t CRC_POLY;
extern volatile uint32_t CRM_CTRL;
extern volatile uint32_t CRM_PLLCFG;
extern volatile uint32_t CRM_CFG;
extern volatile uint32_t CRM_CLKINT;
extern volatile uint32_t CRM_AHBRST1;
extern volatile uint32_t CRM_AHBRST2;
extern volatile uint32_t CRM_AHBRST3;
extern volatile uint32_t CRM_APB1RST;
extern volatile uint32_t CRM_APB2RST;
extern volatile uint32_t CRM_AHBEN1;
extern volatile uint32_t CRM_AHBEN2;
extern volatile uint32_t CRM_AHBEN3;
extern volatile uint32_t CRM_APB1EN;
extern volatile uint32_t CRM_APB2EN;
extern volatile uint32_t CRM_AHBLPEN1;
extern volatile uint32_t CRM_AHBLPEN2;
extern volatile uint32_t CRM_AHBLPEN3;
extern volatile uint32_t CRM_APB1LPEN;
extern volatile uint32_t CRM_APB2LPEN;
extern volatile uint32_t CRM_BPDC;
extern volatile uint32_t CRM_CTRLSTS;
extern volatile uint32_t CRM_OTGHS;
extern volatile uint32_t CRM_MISC1;
extern volatile uint32_t CRM_MISC2;
extern volatile uint32_t FLASH_PSR;
extern volatile uint32_t FLASH_UNLOCK;
extern volatile uint32_t FLASH_USD_UNLOCK;
extern volatile uint32_t FLASH_STS;
extern volatile uint32_t FLASH_CTRL;
extern volatile uint32_t FLASH_ADDR;
extern volatile uint32_t FLASH_USD;
extern volatile uint32_t FLASH_EPPS;
extern volatile uint32_t FLASH_SLIB_STS0;
extern volatile uint32_t FLASH_SLIB_STS1;
extern volatile uint32_t FLASH_SLIB_PWD_CLR;
extern volatile uint32_t FLASH_SLIB_MISC_STS;
extern volatile uint32_t FLASH_CRC_ADDR;
extern volatile uint32_t FLASH_CRC_CTRL;
extern volatile uint32_t FLASH_CRC_CHKR;
extern volatile uint32_t FLASH_SLIB_SET_PWD;
extern volatile uint32_t FLASH_SLIB_SET_RANGE;
extern volatile uint32_t FLASH_EM_SLIB_SET;
extern volatile uint32_t FLASH_BTM_MODE_SET;
extern volatile uint32_t FLASH_SLIB_UNLOCK;
extern volatile uint32_t DMA1_STS;
extern volatile uint32_t DMA1_CLR;
extern volatile uint32_t DMA1_C1CTRL;
extern volatile uint32_t DMA1_C1DTCNT;
extern volatile uint32_t DMA1_C1PADDR;
extern volatile uint32_t DMA1_C1MADDR;
extern volatile uint32_t DMA1_C2CTRL;
extern volatile uint32_t DMA1_C2DTCNT;
extern volatile uint32_t DMA1_C2PADDR;
extern volatile uint32_t DMA1_C2MADDR;
extern volatile uint32_t DMA1_C3CTRL;
extern volatile uint32_t DMA1_C3DTCNT;
extern volatile uint32_t DMA1_C3PADDR;
extern volatile uint32_t DMA1_C3MADDR;
extern volatile uint32_t DMA1_C4CTRL;
extern volatile uint32_t DMA1_C4DTCNT;
extern volatile uint32_t DMA1_C4PADDR;
extern volatile uint32_t DMA1_C4MADDR;
extern volatile uint32_t DMA1_C5CTRL;
extern volatile uint32_t DMA1_C5DTCNT;
extern volatile uint32_t DMA1_C5PADDR;
extern volatile uint32_t DMA1_C5MADDR;
extern volatile uint32_t DMA1_C6CTRL;
extern volatile uint32_t DMA1_C6DTCNT;
extern volatile uint32_t DMA1_C6PADDR;
extern volatile uint32_t DMA1_C6MADDR;
extern volatile uint32_t DMA1_C7CTRL;
extern volatile uint32_t DMA1_C7DTCNT;
extern volatile uint32_t DMA1_C7PADDR;
extern volatile uint32_t DMA1_C7MADDR;
extern volatile uint32_t DMA1_DMA_MUXSEL;
extern volatile uint32_t DMA1_MUXC1CTRL;
extern volatile uint32_t DMA1_MUXC2CTRL;
extern volatile uint32_t DMA1_MUXC3CTRL;
extern volatile uint32_t DMA1_MUXC4CTRL;
extern volatile uint32_t DMA1_MUXC5CTRL;
extern volatile uint32_t DMA1_MUXC6CTRL;
extern volatile uint32_t DMA1_MUXC7CTRL;
extern volatile uint32_t DMA1_MUXG1CTRL;
extern volatile uint32_t DMA1_MUXG2CTRL;
extern volatile uint32_t DMA1_MUXG3CTRL;
extern volatile uint32_t DMA1_MUXG4CTRL;
extern volatile uint32_t DMA1_MUXSYNCSTS;
extern volatile uint32_t DMA1_MUXSYNCCLR;
extern volatile uint32_t DMA1_MUXGSTS;
extern volatile uint32_t DMA1_MUXGCLR;
extern volatile uint32_t DMA2_STS;
extern volatile uint32_t DMA2_CLR;
extern volatile uint32_t DMA2_C1CTRL;
extern volatile uint32_t DMA2_C1DTCNT;
extern volatile uint32_t DMA2_C1PADDR;
extern volatile uint32_t DMA2_C1MADDR;
extern volatile uint32_t DMA2_C2CTRL;
extern volatile uint32_t DMA2_C2DTCNT;
extern volatile uint32_t DMA2_C2PADDR;
extern volatile uint32_t DMA2_C2MADDR;
extern volatile uint32_t DMA2_C3CTRL;
extern volatile uint32_t DMA2_C3DTCNT;
extern volatile uint32_t DMA2_C3PADDR;
extern volatile uint32_t DMA2_C3MADDR;
extern volatile uint32_t DMA2_C4CTRL;
extern volatile uint32_t DMA2_C4DTCNT;
extern volatile uint32_t DMA2_C4PADDR;
extern volatile uint32_t DMA2_C4MADDR;
extern volatile uint32_t DMA2_C5CTRL;
extern volatile uint32_t DMA2_C5DTCNT;
extern volatile uint32_t DMA2_C5PADDR;
extern volatile uint32_t DMA2_C5MADDR;
extern volatile uint32_t DMA2_C6CTRL;
extern volatile uint32_t DMA2_C6DTCNT;
extern volatile uint32_t DMA2_C6PADDR;
extern volatile uint32_t DMA2_C6MADDR;
extern volatile uint32_t DMA2_C7CTRL;
extern volatile uint32_t DMA2_C7DTCNT;
extern volatile uint32_t DMA2_C7PADDR;
extern volatile uint32_t DMA2_C7MADDR;
extern volatile uint32_t DMA2_DMA_MUXSEL;
extern volatile uint32_t DMA2_MUXC1CTRL;
extern volatile uint32_t DMA2_MUXC2CTRL;
extern volatile uint32_t DMA2_MUXC3CTRL;
extern volatile uint32_t DMA2_MUXC4CTRL;
extern volatile uint32_t DMA2_MUXC5CTRL;
extern volatile uint32_t DMA2_MUXC6CTRL;
extern volatile uint32_t DMA2_MUXC7CTRL;
extern volatile uint32_t DMA2_MUXG1CTRL;
extern volatile uint32_t DMA2_MUXG2CTRL;
extern volatile uint32_t DMA2_MUXG3CTRL;
extern volatile uint32_t DMA2_MUXG4CTRL;
extern volatile uint32_t DMA2_MUXSYNCSTS;
extern volatile uint32_t DMA2_MUXSYNCCLR;
extern volatile uint32_t DMA2_MUXGSTS;
extern volatile uint32_t DMA2_MUXGCLR;
extern volatile uint32_t SDIO1_PWRCTRL;
extern volatile uint32_t SDIO1_CLKCTRL;
extern volatile uint32_t SDIO1_ARGU;
extern volatile uint32_t SDIO1_CMDCTRL;
extern volatile uint32_t SDIO1_RSPCMD;
extern volatile uint32_t SDIO1_RSP1;
extern volatile uint32_t SDIO1_RSP2;
extern volatile uint32_t SDIO1_RSP3;
extern volatile uint32_t SDIO1_RSP4;
extern volatile uint32_t SDIO1_DTTMR;
extern volatile uint32_t SDIO1_DTLEN;
extern volatile uint32_t SDIO1_DTCTRL;
extern volatile uint32_t SDIO1_DTCNT;
extern volatile uint32_t SDIO1_STS;
extern volatile uint32_t SDIO1_INTCLR;
extern volatile uint32_t SDIO1_INTEN;
extern volatile uint32_t SDIO1_BUFCNT;
extern volatile uint32_t SDIO1_BUF;
extern volatile uint32_t USB_OTGHS_GLOBAL_GOTGCTL;
extern volatile uint32_t USB_OTGHS_GLOBAL_GOTGINT;
extern volatile uint32_t USB_OTGHS_GLOBAL_GAHBCFG;
extern volatile uint32_t USB_OTGHS_GLOBAL_GUSBCFG;
extern volatile uint32_t USB_OTGHS_GLOBAL_GRSTCTL;
extern volatile uint32_t USB_OTGHS_GLOBAL_GINTSTS;
extern volatile uint32_t USB_OTGHS_GLOBAL_GINTMSK;
extern volatile uint32_t USB_OTGHS_GLOBAL_GRXSTSR_Host;
extern volatile uint32_t USB_OTGHS_GLOBAL_GRXFSIZ;
extern volatile uint32_t USB_OTGHS_GLOBAL_GNPTXFSIZ;
extern volatile uint32_t USB_OTGHS_GLOBAL_GNPTXSTS;
extern volatile uint32_t USB_OTGHS_GLOBAL_GCCFG;
extern volatile uint32_t USB_OTGHS_GLOBAL_GUID;
extern volatile uint32_t USB_OTGHS_GLOBAL_HPTXFSIZ;
extern volatile uint32_t USB_OTGHS_GLOBAL_DIEPTXF1;
extern volatile uint32_t USB_OTGHS_GLOBAL_DIEPTXF2;
extern volatile uint32_t USB_OTGHS_GLOBAL_DIEPTXF3;
extern volatile uint32_t USB_OTGHS_GLOBAL_DIEPTXF4;
extern volatile uint32_t USB_OTGHS_GLOBAL_DIEPTXF5;
extern volatile uint32_t USB_OTGHS_GLOBAL_DIEPTXF6;
extern volatile uint32_t USB_OTGHS_GLOBAL_DIEPTXF7;
extern volatile uint32_t USB_OTGHS_HOST_HCFG;
extern volatile uint32_t USB_OTGHS_HOST_HFIR;
extern volatile uint32_t USB_OTGHS_HOST_HFNUM;
extern volatile uint32_t USB_OTGHS_HOST_HPTXSTS;
extern volatile uint32_t USB_OTGHS_HOST_HAINT;
extern volatile uint32_t USB_OTGHS_HOST_HAINTMSK;
extern volatile uint32_t USB_OTGHS_HOST_HPRT;
extern volatile uint32_t USB_OTGHS_HOST_HCCHAR0;
extern volatile uint32_t USB_OTGHS_HOST_HCSPLT0;
extern volatile uint32_t USB_OTGHS_HOST_HCINT0;
extern volatile uint32_t USB_OTGHS_HOST_HCINTMSK0;
extern volatile uint32_t USB_OTGHS_HOST_HCTSIZ0;
extern volatile uint32_t USB_OTGHS_HOST_HCDMA0;
extern volatile uint32_t USB_OTGHS_HOST_HCCHAR1;
extern volatile uint32_t USB_OTGHS_HOST_HCSPLT1;
extern volatile uint32_t USB_OTGHS_HOST_HCINT1;
extern volatile uint32_t USB_OTGHS_HOST_HCINTMSK1;
extern volatile uint32_t USB_OTGHS_HOST_HCTSIZ1;
extern volatile uint32_t USB_OTGHS_HOST_HCDMA1;
extern volatile uint32_t USB_OTGHS_HOST_HCCHAR2;
extern volatile uint32_t USB_OTGHS_HOST_HCSPLT2;
extern volatile uint32_t USB_OTGHS_HOST_HCINT2;
extern volatile uint32_t USB_OTGHS_HOST_HCINTMSK2;
extern volatile uint32_t USB_OTGHS_HOST_HCTSIZ2;
extern volatile uint32_t USB_OTGHS_HOST_HCDMA2;
extern volatile uint32_t USB_OTGHS_HOST_HCCHAR3;
extern volatile uint32_t USB_OTGHS_HOST_HCSPLT3;
extern volatile uint32_t USB_OTGHS_HOST_HCINT3;
extern volatile uint32_t USB_OTGHS_HOST_HCINTMSK3;
extern volatile uint32_t USB_OTGHS_HOST_HCTSIZ3;
extern volatile uint32_t USB_OTGHS_HOST_HCDMA3;
extern volatile uint32_t USB_OTGHS_HOST_HCCHAR4;
extern volatile uint32_t USB_OTGHS_HOST_HCSPLT4;
extern volatile uint32_t USB_OTGHS_HOST_HCINT4;
extern volatile uint32_t USB_OTGHS_HOST_HCINTMSK4;
extern volatile uint32_t USB_OTGHS_HOST_HCTSIZ4;
extern volatile uint32_t USB_OTGHS_HOST_HCDMA4;
extern volatile uint32_t USB_OTGHS_HOST_HCCHAR5;
extern volatile uint32_t USB_OTGHS_HOST_HCSPLT5;
extern volatile uint32_t USB_OTGHS_HOST_HCINT5;
extern volatile uint32_t USB_OTGHS_HOST_HCINTMSK5;
extern volatile uint32_t USB_OTGHS_HOST_HCTSIZ5;
extern volatile uint32_t USB_OTGHS_HOST_HCDMA5;
extern volatile uint32_t USB_OTGHS_HOST_HCCHAR6;
extern volatile uint32_t USB_OTGHS_HOST_HCSPLT6;
extern volatile uint32_t USB_OTGHS_HOST_HCINT6;
extern volatile uint32_t USB_OTGHS_HOST_HCINTMSK6;
extern volatile uint32_t USB_OTGHS_HOST_HCTSIZ6;
extern volatile uint32_t USB_OTGHS_HOST_HCDMA6;
extern volatile uint32_t USB_OTGHS_HOST_HCCHAR7;
extern volatile uint32_t USB_OTGHS_HOST_HCSPLT7;
extern volatile uint32_t USB_OTGHS_HOST_HCINT7;
extern volatile uint32_t USB_OTGHS_HOST_HCINTMSK7;
extern volatile uint32_t USB_OTGHS_HOST_HCTSIZ7;
extern volatile uint32_t USB_OTGHS_HOST_HCDMA7;
extern volatile uint32_t USB_OTGHS_HOST_HCCHAR8;
extern volatile uint32_t USB_OTGHS_HOST_HCSPLT8;
extern volatile uint32_t USB_OTGHS_HOST_HCINT8;
extern volatile uint32_t USB_OTGHS_HOST_HCINTMSK8;
extern volatile uint32_t USB_OTGHS_HOST_HCTSIZ8;
extern volatile uint32_t USB_OTGHS_HOST_HCDMA8;
extern volatile uint32_t USB_OTGHS_HOST_HCCHAR9;
extern volatile uint32_t USB_OTGHS_HOST_HCSPLT9;
extern volatile uint32_t USB_OTGHS_HOST_HCINT9;
extern volatile uint32_t USB_OTGHS_HOST_HCINTMSK9;
extern volatile uint32_t USB_OTGHS_HOST_HCTSIZ9;
extern volatile uint32_t USB_OTGHS_HOST_HCDMA9;
extern volatile uint32_t USB_OTGHS_HOST_HCCHAR10;
extern volatile uint32_t USB_OTGHS_HOST_HCSPLT10;
extern volatile uint32_t USB_OTGHS_HOST_HCINT10;
extern volatile uint32_t USB_OTGHS_HOST_HCINTMSK10;
extern volatile uint32_t USB_OTGHS_HOST_HCTSIZ10;
extern volatile uint32_t USB_OTGHS_HOST_HCDMA10;
extern volatile uint32_t USB_OTGHS_HOST_HCCHAR11;
extern volatile uint32_t USB_OTGHS_HOST_HCSPLT11;
extern volatile uint32_t USB_OTGHS_HOST_HCINT11;
extern volatile uint32_t USB_OTGHS_HOST_HCINTMSK11;
extern volatile uint32_t USB_OTGHS_HOST_HCTSIZ11;
extern volatile uint32_t USB_OTGHS_HOST_HCDMA11;
extern volatile uint32_t USB_OTGHS_HOST_HCCHAR12;
extern volatile uint32_t USB_OTGHS_HOST_HCSPLT12;
extern volatile uint32_t USB_OTGHS_HOST_HCINT12;
extern volatile uint32_t USB_OTGHS_HOST_HCINTMSK12;
extern volatile uint32_t USB_OTGHS_HOST_HCTSIZ12;
extern volatile uint32_t USB_OTGHS_HOST_HCDMA12;
extern volatile uint32_t USB_OTGHS_HOST_HCCHAR13;
extern volatile uint32_t USB_OTGHS_HOST_HCSPLT13;
extern volatile uint32_t USB_OTGHS_HOST_HCINT13;
extern volatile uint32_t USB_OTGHS_HOST_HCINTMSK13;
extern volatile uint32_t USB_OTGHS_HOST_HCTSIZ13;
extern volatile uint32_t USB_OTGHS_HOST_HCDMA13;
extern volatile uint32_t USB_OTGHS_HOST_HCCHAR14;
extern volatile uint32_t USB_OTGHS_HOST_HCSPLT14;
extern volatile uint32_t USB_OTGHS_HOST_HCINT14;
extern volatile uint32_t USB_OTGHS_HOST_HCINTMSK14;
extern volatile uint32_t USB_OTGHS_HOST_HCTSIZ14;
extern volatile uint32_t USB_OTGHS_HOST_HCDMA14;
extern volatile uint32_t USB_OTGHS_HOST_HCCHAR15;
extern volatile uint32_t USB_OTGHS_HOST_HCSPLT15;
extern volatile uint32_t USB_OTGHS_HOST_HCINT15;
extern volatile uint32_t USB_OTGHS_HOST_HCINTMSK15;
extern volatile uint32_t USB_OTGHS_HOST_HCTSIZ15;
extern volatile uint32_t USB_OTGHS_HOST_HCDMA15;
extern volatile uint32_t USB_OTGHS_DEVICE_DCFG;
extern volatile uint32_t USB_OTGHS_DEVICE_DCTL;
extern volatile uint32_t USB_OTGHS_DEVICE_DSTS;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPMSK;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPMSK;
extern volatile uint32_t USB_OTGHS_DEVICE_DAINT;
extern volatile uint32_t USB_OTGHS_DEVICE_DAINTMSK;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPEMPMSK;
extern volatile uint32_t USB_OTGHS_DEVICE_DEACHINT;
extern volatile uint32_t USB_OTGHS_DEVICE_DEACHINTMSK;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPEACHMSK1;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPEACHMSK1;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPCTL0;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPINT0;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPTSIZ0;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPDMA0;
extern volatile uint32_t USB_OTGHS_DEVICE_DTXFSTS0;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPCTL1;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPINT1;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPTSIZ1;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPDMA1;
extern volatile uint32_t USB_OTGHS_DEVICE_DTXFSTS1;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPCTL2;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPINT2;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPTSIZ2;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPDMA2;
extern volatile uint32_t USB_OTGHS_DEVICE_DTXFSTS2;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPCTL3;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPINT3;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPTSIZ3;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPDMA3;
extern volatile uint32_t USB_OTGHS_DEVICE_DTXFSTS3;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPCTL4;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPINT4;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPTSIZ4;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPDMA4;
extern volatile uint32_t USB_OTGHS_DEVICE_DTXFSTS4;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPCTL5;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPINT5;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPTSIZ5;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPDMA5;
extern volatile uint32_t USB_OTGHS_DEVICE_DTXFSTS5;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPCTL6;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPINT6;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPTSIZ6;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPDMA6;
extern volatile uint32_t USB_OTGHS_DEVICE_DTXFSTS6;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPCTL7;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPINT7;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPTSIZ7;
extern volatile uint32_t USB_OTGHS_DEVICE_DIEPDMA7;
extern volatile uint32_t USB_OTGHS_DEVICE_DTXFSTS7;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPCTL0;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPINT0;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPTSIZ0;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPDMA0;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPCTL1;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPINT1;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPTSIZ1;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPDMA1;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPCTL2;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPINT2;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPTSIZ2;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPDMA2;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPCTL3;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPINT3;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPTSIZ3;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPDMA3;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPCTL4;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPINT4;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPTSIZ4;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPDMA4;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPCTL5;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPINT5;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPTSIZ5;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPDMA5;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPCTL6;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPINT6;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPTSIZ6;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPDMA6;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPCTL7;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPINT7;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPTSIZ7;
extern volatile uint32_t USB_OTGHS_DEVICE_DOEPDMA7;
extern volatile uint32_t USB_OTGHS_PWRCLK_PCGCCTL;
extern volatile uint32_t USB_OTGFS_GLOBAL_GOTGCTL;
extern volatile uint32_t USB_OTG_BASE;
extern volatile uint32_t USB_OTGFS_GLOBAL_GOTGINT;
extern volatile uint32_t USB_OTGFS_GLOBAL_GAHBCFG;
extern volatile uint32_t USB_OTGFS_GLOBAL_GUSBCFG;
extern volatile uint32_t USB_OTGFS_GLOBAL_GRSTCTL;
extern volatile uint32_t USB_OTGFS_GLOBAL_GINTSTS;
extern volatile uint32_t USB_OTGFS_GLOBAL_GINTMSK;
extern volatile uint32_t USB_OTGFS_GLOBAL_GRXSTSR_Host;
extern volatile uint32_t USB_OTGFS_GLOBAL_GRXFSIZ;
extern volatile uint32_t USB_OTGFS_GLOBAL_GNPTXFSIZ;
extern volatile uint32_t USB_OTGFS_GLOBAL_GNPTXSTS;
extern volatile uint32_t USB_OTGFS_GLOBAL_GCCFG;
extern volatile uint32_t USB_OTGFS_GLOBAL_GUID;
extern volatile uint32_t USB_OTGFS_GLOBAL_HPTXFSIZ;
extern volatile uint32_t USB_OTGFS_GLOBAL_DIEPTXF1;
extern volatile uint32_t USB_OTGFS_GLOBAL_DIEPTXF2;
extern volatile uint32_t USB_OTGFS_GLOBAL_DIEPTXF3;
extern volatile uint32_t USB_OTGFS_GLOBAL_DIEPTXF4;
extern volatile uint32_t USB_OTGFS_GLOBAL_DIEPTXF5;
extern volatile uint32_t USB_OTGFS_GLOBAL_DIEPTXF6;
extern volatile uint32_t USB_OTGFS_GLOBAL_DIEPTXF7;
extern volatile uint32_t USB_OTG1_HOST_HCFG;
extern volatile uint32_t USB_OTG1_HOST_HFIR;
extern volatile uint32_t USB_OTG1_HOST_HFNUM;
extern volatile uint32_t USB_OTG1_HOST_HPTXSTS;
extern volatile uint32_t USB_OTG1_HOST_HAINT;
extern volatile uint32_t USB_OTG1_HOST_HAINTMSK;
extern volatile uint32_t USB_OTG1_HOST_HPRT;
extern volatile uint32_t USB_OTG1_HOST_HCCHAR0;
extern volatile uint32_t USB_OTG1_HOST_HCINT0;
extern volatile uint32_t USB_OTG1_HOST_HCINTMSK0;
extern volatile uint32_t USB_OTG1_HOST_HCTSIZ0;
extern volatile uint32_t USB_OTG1_HOST_HCCHAR1;
extern volatile uint32_t USB_OTG1_HOST_HCINT1;
extern volatile uint32_t USB_OTG1_HOST_HCINTMSK1;
extern volatile uint32_t USB_OTG1_HOST_HCTSIZ1;
extern volatile uint32_t USB_OTG1_HOST_HCCHAR2;
extern volatile uint32_t USB_OTG1_HOST_HCINT2;
extern volatile uint32_t USB_OTG1_HOST_HCINTMSK2;
extern volatile uint32_t USB_OTG1_HOST_HCTSIZ2;
extern volatile uint32_t USB_OTG1_HOST_HCCHAR3;
extern volatile uint32_t USB_OTG1_HOST_HCINT3;
extern volatile uint32_t USB_OTG1_HOST_HCINTMSK3;
extern volatile uint32_t USB_OTG1_HOST_HCTSIZ3;
extern volatile uint32_t USB_OTG1_HOST_HCCHAR4;
extern volatile uint32_t USB_OTG1_HOST_HCINT4;
extern volatile uint32_t USB_OTG1_HOST_HCINTMSK4;
extern volatile uint32_t USB_OTG1_HOST_HCTSIZ4;
extern volatile uint32_t USB_OTG1_HOST_HCCHAR5;
extern volatile uint32_t USB_OTG1_HOST_HCINT5;
extern volatile uint32_t USB_OTG1_HOST_HCINTMSK5;
extern volatile uint32_t USB_OTG1_HOST_HCTSIZ5;
extern volatile uint32_t USB_OTG1_HOST_HCCHAR6;
extern volatile uint32_t USB_OTG1_HOST_HCINT6;
extern volatile uint32_t USB_OTG1_HOST_HCINTMSK6;
extern volatile uint32_t USB_OTG1_HOST_HCTSIZ6;
extern volatile uint32_t USB_OTG1_HOST_HCCHAR7;
extern volatile uint32_t USB_OTG1_HOST_HCINT7;
extern volatile uint32_t USB_OTG1_HOST_HCINTMSK7;
extern volatile uint32_t USB_OTG1_HOST_HCTSIZ7;
extern volatile uint32_t USB_OTG1_HOST_HCCHAR8;
extern volatile uint32_t USB_OTG1_HOST_HCINT8;
extern volatile uint32_t USB_OTG1_HOST_HCINTMSK8;
extern volatile uint32_t USB_OTG1_HOST_HCTSIZ8;
extern volatile uint32_t USB_OTG1_HOST_HCCHAR9;
extern volatile uint32_t USB_OTG1_HOST_HCINT9;
extern volatile uint32_t USB_OTG1_HOST_HCINTMSK9;
extern volatile uint32_t USB_OTG1_HOST_HCTSIZ9;
extern volatile uint32_t USB_OTG1_HOST_HCCHAR10;
extern volatile uint32_t USB_OTG1_HOST_HCINT10;
extern volatile uint32_t USB_OTG1_HOST_HCINTMSK10;
extern volatile uint32_t USB_OTG1_HOST_HCTSIZ10;
extern volatile uint32_t USB_OTG1_HOST_HCCHAR11;
extern volatile uint32_t USB_OTG1_HOST_HCINT11;
extern volatile uint32_t USB_OTG1_HOST_HCINTMSK11;
extern volatile uint32_t USB_OTG1_HOST_HCTSIZ11;
extern volatile uint32_t USB_OTG1_HOST_HCCHAR12;
extern volatile uint32_t USB_OTG1_HOST_HCINT12;
extern volatile uint32_t USB_OTG1_HOST_HCINTMSK12;
extern volatile uint32_t USB_OTG1_HOST_HCTSIZ12;
extern volatile uint32_t USB_OTG1_HOST_HCCHAR13;
extern volatile uint32_t USB_OTG1_HOST_HCINT13;
extern volatile uint32_t USB_OTG1_HOST_HCINTMSK13;
extern volatile uint32_t USB_OTG1_HOST_HCTSIZ13;
extern volatile uint32_t USB_OTG1_HOST_HCCHAR14;
extern volatile uint32_t USB_OTG1_HOST_HCINT14;
extern volatile uint32_t USB_OTG1_HOST_HCINTMSK14;
extern volatile uint32_t USB_OTG1_HOST_HCTSIZ14;
extern volatile uint32_t USB_OTG1_HOST_HCCHAR15;
extern volatile uint32_t USB_OTG1_HOST_HCINT15;
extern volatile uint32_t USB_OTG1_HOST_HCINTMSK15;
extern volatile uint32_t USB_OTG1_HOST_HCTSIZ15;
extern volatile uint32_t USB_OTG1_DEVICE_DCFG;
extern volatile uint32_t USB_OTG1_DEVICE_DCTL;
extern volatile uint32_t USB_OTG1_DEVICE_DSTS;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPMSK;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPMSK;
extern volatile uint32_t USB_OTG1_DEVICE_DAINT;
extern volatile uint32_t USB_OTG1_DEVICE_DAINTMSK;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPEMPMSK;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPCTL0;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPINT0;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPTSIZ0;
extern volatile uint32_t USB_OTG1_DEVICE_DTXFSTS0;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPCTL1;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPINT1;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPTSIZ1;
extern volatile uint32_t USB_OTG1_DEVICE_DTXFSTS1;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPCTL2;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPINT2;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPTSIZ2;
extern volatile uint32_t USB_OTG1_DEVICE_DTXFSTS2;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPCTL3;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPINT3;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPTSIZ3;
extern volatile uint32_t USB_OTG1_DEVICE_DTXFSTS3;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPCTL4;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPINT4;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPTSIZ4;
extern volatile uint32_t USB_OTG1_DEVICE_DTXFSTS4;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPCTL5;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPINT5;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPTSIZ5;
extern volatile uint32_t USB_OTG1_DEVICE_DTXFSTS5;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPCTL6;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPINT6;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPTSIZ6;
extern volatile uint32_t USB_OTG1_DEVICE_DTXFSTS6;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPCTL7;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPINT7;
extern volatile uint32_t USB_OTG1_DEVICE_DIEPTSIZ7;
extern volatile uint32_t USB_OTG1_DEVICE_DTXFSTS7;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPCTL0;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPINT0;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPTSIZ0;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPCTL1;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPINT1;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPTSIZ1;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPCTL2;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPINT2;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPTSIZ2;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPCTL3;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPINT3;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPTSIZ3;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPCTL4;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPINT4;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPTSIZ4;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPCTL5;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPINT5;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPTSIZ5;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPCTL6;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPINT6;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPTSIZ6;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPCTL7;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPINT7;
extern volatile uint32_t USB_OTG1_DEVICE_DOEPTSIZ7;
extern volatile uint32_t USB_OTG1_PWRCLK_PCGCCTL;
extern volatile uint32_t DVP_CTRL;
extern volatile uint32_t DVP_STS;
extern volatile uint32_t DVP_ESTS;
extern volatile uint32_t DVP_IENA;
extern volatile uint32_t DVP_ISTS;
extern volatile uint32_t DVP_ICLR;
extern volatile uint32_t DVP_SCR;
extern volatile uint32_t DVP_SUR;
extern volatile uint32_t DVP_CWST;
extern volatile uint32_t DVP_CWSZ;
extern volatile uint32_t DVP_DT;
extern volatile uint32_t DVP_ACTRL;
extern volatile uint32_t DVP_HSCF;
extern volatile uint32_t DVP_VSCF;
extern volatile uint32_t DVP_FRF;
extern volatile uint32_t DVP_BTH;
extern volatile uint32_t SDIO2_PWRCTRL;
extern volatile uint32_t SDIO2_CLKCTRL;
extern volatile uint32_t SDIO2_ARGU;
extern volatile uint32_t SDIO2_CMDCTRL;
extern volatile uint32_t SDIO2_RSPCMD;
extern volatile uint32_t SDIO2_RSP1;
extern volatile uint32_t SDIO2_RSP2;
extern volatile uint32_t SDIO2_RSP3;
extern volatile uint32_t SDIO2_RSP4;
extern volatile uint32_t SDIO2_DTTMR;
extern volatile uint32_t SDIO2_DTLEN;
extern volatile uint32_t SDIO2_DTCTRL;
extern volatile uint32_t SDIO2_DTCNT;
extern volatile uint32_t SDIO2_STS;
extern volatile uint32_t SDIO2_INTCLR;
extern volatile uint32_t SDIO2_INTEN;
extern volatile uint32_t SDIO2_BUFCNT;
extern volatile uint32_t SDIO2_BUF;

/* ── Firmware functions ────────────────────────────────────────────────── */
void crt0_startup(void);
void crt0_decompress_data(void);
void crt0_zero_bss(void);
void crt0_enable_fpu_and_prng(void);
void crt0_early_init_stub(void);
void crt0_init_bss_data(void);
uint32_t prng_next(uint32_t * state);
void Reset_Handler(void);
void Default_Handler(void);
void prng_get_constants(void);
uint32_t __aeabi_uldiv(uint32_t dividend, uint32_t divisor);
void prng_seed(uint32_t seed);
void prng_seed_default(void);
void * memcpy(void * dst, void * src, uint32_t n);
void * rt_memcpy(void * dst, void * src, uint32_t n);
void * rt_memset(void * dst, uint32_t n, uint32_t val);
void memset_zero_aligned(void * dst, uint32_t n);
void memset_zero(void * dst, uint32_t n);
void prng_init_state(void);
int32_t prng_rand(void);
void * prng_get_state_ptr(void);
void adc_sensor_process(void);
void adc_sample_average_send(void);
void BusFault_Handler(void);
void get_multi_magnetism(void);
void set_multi_magnetism(void);
void cmd_set_fn_layer(void);
void cmd_set_keymatrix(void);
void cmd_set_macro(void);
void cmd_set_audio_viz(void);
void led_pwm_ramp(void);
void led_breathe_step(void);
void key_debounce_process(void);
void wireless_rx_dma_handler(void);
void adc_dma_complete_handler(void);
void DebugMon_Handler(void);
void ws2812_set_pixel(uint32_t led_index, uint32_t brightness, uint32_t red, uint32_t green, uint32_t blue);
void rtc_wakeup_handler(void);
void config_save_apply(void);
uint32_t keymap_lookup(uint32_t default_key, uint32_t layer, uint32_t key_index, uint32_t pressed);
void flash_save_fn_layer(void);
void HardFault_Handler(void);
void hid_key_release(uint32_t keycode);
void hid_key_press(uint32_t keycode);
void hid_nkro_key_set(uint32_t keycode, uint32_t pressed);
void key_setting_adjust(uint32_t key_fn, uint32_t pressed);
void rgb_led_animate(void);
void fn_layer_state_update(uint32_t key_index, uint32_t pressed);
void state_reset_all(void);
void led_effect_firework(void);
void led_effect_twinkle(void);
void led_effect_reactive_multi_ripple(void);
void led_effect_cross_scan(void);
void led_effect_reactive_ripple(void);
void led_effect_helix(void);
void led_effect_raindrop(void);
void led_effect_audio_viz(void);
void led_effect_breathe(void);
void led_effect_flag_pattern(void);
void key_matrix_scan(void);
void led_effect_ripple_auto(void);
void led_effect_snake(void);
void led_ws2812_color_step(void);
void led_effect_static(void);
void led_effect_spiral(void);
void led_effect_user_picture(void);
void led_effect_wave_vertical(void);
void led_effect_wave(void);
void key_action_dispatch(void);
void macro_start_playback(uint32_t macro_id);
void macro_playback_tick(void);
void apply_config_changes(void);
void flash_save_macro(void);
void flash_save_switch_calibration(void);
void flash_save_switch_thresholds(void);
void hall_sensor_key_eval(void);
void key_analog_depth_calc(void);
void key_actuation_eval(void);
void flash_save_magnetism(void);
void flash_load_switch_config(void);
void mag_calibration_load_or_init(void);
void mag_calibration_reset_all(void);
void flash_save_dks_if_dirty(void);
void flash_save_dks_state(void);
void key_rt_custom_mode(void);
void key_per_switch_state_machine(void);
void MemManage_Handler(void);
void rf_led_indicator_send(void);
void mouse_action_dispatch(uint32_t action, uint32_t pressed);
void mouse_move_set(uint32_t axis, int32_t delta);
void NMI_Handler(void);
void OTGFS1_IRQHandler(void);
void PendSV_Handler(void);
void key_event_process(void);
void keycode_dispatch(uint32_t keycode, uint32_t pressed);
uint32_t game_mode_key_check(uint32_t key_index);
void config_load_all(void);
void hid_report_check_send(void);
void hid_nkro_report_clear(void);
void hid_6kro_report_clear(void);
void hid_mouse_report_clear(void);
void hid_6kro_report_queue(void);
void config_factory_reset(void);
void SVCall_Handler(void);
void bt_event_queue(uint32_t event);
void send_depth_monitor_report(void);
void led_ripple_effect(void);
void SysTick_Handler(void);
void system_clock_init(void);
void tmr4_scan_tick_poll(void);
void fast_tick_8khz_handler(void);
void fn_key_mode_switch(void);
void keyscan_detect_connection_mode(void);
void connection_mode_gpio_set(uint32_t mode);
void usb_ep_report_send(void);
void vendor_command_dispatch(void);
void UsageFault_Handler(void);
void flash_save_userpic(void);
void flash_save_config(void);
void dma_struct_default(uint32_t * dma_struct);
void crt0_stub(void);
void adc_channel_sequence_init(void);
void adc_common_init(uint32_t * init_struct);
void adc_common_struct_default(uint32_t * init_struct);
void adc_calibration_reset(void);
bool adc_calibration_reset_busy(void);
void adc_calibration_start(void);
bool adc_calibration_busy(void);
void adc_prescaler_set(uint32_t prescaler);
void dma_adc_config(void);
void adc_dma_enable(uint32_t enable);
void adc_enable(uint32_t enable);
void adc_regular_channel_set(uint32_t * seq_struct);
void adc_external_trigger_set(uint32_t trigger, uint32_t enable);
void gpio_timer_init(void);
void nvic_system_reset(void);
void usb_phy_pll_init(void);
void usb_ep2_in_transmit(void * buf, uint32_t len);
void usb_suspend_handler(void);
void hid_class_out_handler(void * udev, uint32_t ep_num);
void hid_class_setup_handler(otg_dev_handle_t * udev, usb_setup_pkt_t * setup_pkt);
void crm_adc_clock_div_set(uint32_t div);
void adc_sample_time_set(uint32_t time);
void adc_ordinary_channel_set(uint32_t channel);
void crm_flash_wait_config(uint32_t wait_states);
void crm_otgfs_clock_enable(uint32_t enable);
void crm_clock_enable(uint32_t periph, uint32_t enable);
void crm_clocks_freq_get(uint32_t * freq_out);
void crm_usb_clock_source_select(uint32_t mult);
bool crm_flag_status_get(uint32_t flag);
void usb_otg_wait_ahb_idle(void * udev);
void crm_pll_source_config(uint32_t ref_div);
void crm_periph_clock_set(uint32_t periph, uint32_t enable);
void crm_clock_source_enable(uint32_t source, uint32_t enable);
void crm_pll_config(uint32_t ref_clk, uint32_t mult, uint32_t pre_div, uint32_t post_div);
void crm_ahb_div_set(uint32_t div);
void crm_usb48_clock_source_set(uint32_t source);
void crm_system_clock_init(void);
void crm_sclk_select(uint32_t source);
uint32_t crm_sclk_status_get(void);
void crm_usb_clock_div_set(uint32_t div);
void adc_hw_reconfigure(void);
void systick_interval_init(void);
void delay_ms(uint32_t ms);
void systick_delay_us(uint32_t us);
void usb_cable_change_handler(void);
void adc_power_set(uint32_t enable);
void adc_init_struct_default(uint32_t * init_struct);
void adc_flag_clear(uint32_t flag);
bool adc_flag_get(uint32_t flag);
void adc_init_config(uint32_t * init_struct);
void register_bit_set(uint32_t * reg, uint32_t bit, uint32_t value);
bool dma_flag_get(uint32_t flag);
void adc_reset(void);
void dma_mux_sync_enable(uint32_t * channel, uint32_t enable);
void adc_regular_channel_count_set(uint32_t count);
void usb_otg_clock_init(void);
void usb_otg_phy_init(void);
void ertc_time_set(uint32_t hours, uint32_t minutes, uint32_t seconds, uint32_t ampm);
void ertc_alarm_set(uint32_t day, uint32_t time_field);
void ertc_flag_clear(uint32_t flags);
bool flash_status_flag_get(uint32_t flag);
void ertc_bypass_shadow_set(uint32_t enable);
void flash_operation_wait(void);
void flash_operation_enable(void);
bool ertc_interrupt_flag_get(uint32_t flag);
void ertc_reset_to_defaults(void);
void flash_erase_trigger(uint32_t addr);
void crm_ertc_clock_div_set(uint32_t div);
void flash_wait_state_set(uint32_t states);
void flash_write_enable(void);
void crm_periph_clock_init(void);
void crm_periph_clock_enable(uint32_t * config);
void flash_lock(void);
void flash_wait_complete(void);
void flash_migrate_defaults(void);
void flash_read_to_buffer(uint32_t * dst, uint32_t * src, uint32_t words);
void flash_erase_sector(uint32_t addr);
void flash_unlock(void);
void flash_write_word(uint32_t addr, uint32_t data);
void flash_program_bytes(uint32_t dst, void * src, uint32_t len);
void get_fs_config_desc(void);
void get_device_desc(void);
void get_interface_string(void);
void get_lang_id(void);
void get_manufacturer_string(void);
void get_other_speed_desc(void);
void get_product_string(void);
void get_device_qualifier(void);
void get_serial_string(void);
void get_string_buf_desc(void);
void get_hs_config_desc(void);
void tmr_set_period(uint32_t * tmr, uint32_t period);
void tmr_set_div(uint32_t * tmr, uint32_t div);
void gpio_struct_default(uint32_t * gpio_struct);
void gpio_init_pins(uint32_t * gpio_port, uint32_t * init_struct);
bool gpio_input_data_bit_read(uint32_t * reg, uint32_t flag);
void gpio_pin_mux_set(uint32_t * gpio_port, uint32_t pin_source, uint32_t mux_sel);
void battery_indicator_led(void);
void led_flash_all_effect(void);
void led_flash_single_effect(void);
void led_render_frame(void);
void adc_debounce_filter(void);
void firmware_main(void);
void system_clock_switch(uint32_t target);
void nvic_tmr4_init(void);
void nvic_irq_configure(uint32_t irq, uint32_t preempt, uint32_t sub);
void nvic_set_priority_group(uint32_t group);
void scb_set_vtor(uint32_t base, uint32_t offset);
void usb_cable_detect(void);
void connection_mode_detect(void);
void connection_pins_debounce(void);
void adc_calibration_check(void);
void wireless_power_manager(void);
void crm_usb_phy_enable(uint32_t enable);
void cortex_enter_sleep(void);
void tmr_overflow_int_enable(uint32_t * tmr, uint32_t enable);
void clock_restore_pll(void);
void battery_level_monitor(void);
void wireless_sleep_loop(void);
void keyboard_main_loop(void);
void keyboard_main_loop_entry2(void);
void power_sleep_manager(void);
void connection_mode_transition(void);
void set_connection_state(uint32_t state);
void gpio_sleep_reconfig(void);
void gpio_init_struct_default(uint32_t * init_struct);
void dma_set_periph_inc(uint32_t * periph, uint32_t enable);
void tmr_set_cc_dma_select(uint32_t * tmr, uint32_t select);
void tmr_set_channel_buffer_ctrl(uint32_t * tmr, uint32_t enable);
void tmr_set_ctrl2_bit1(uint32_t * tmr, uint32_t enable);
bool tmr_status_flag_get(uint32_t * tmr, uint32_t flag);
void gpio_config_apply(void);
void build_dongle_reports(void);
void key_state_accumulate(void);
void keyscan_hw_config(uint32_t mode);
void adc_scan_count_reconfig(void);
void adc_reinit(void);
void adc_hw_init(void);
void ws2812_hw_init(void);
void status_indicator_leds(void);
void systick_clock_source_set(uint32_t source);
void systick_timer_init(void);
void tmr_scan_tick_init(void);
void tmr_fast_tick_init(void);
void tmr_set_div_and_period(uint32_t * tmr, uint32_t div, uint32_t period);
void tmr_set_clkdiv(uint32_t * tmr, uint32_t div);
void tmr_set_count_mode(uint32_t * tmr, uint32_t mode);
void tmr_enable(uint32_t * tmr, uint32_t enable);
void periph_flag_clear(uint32_t * reg, uint32_t flag);
bool periph_flag_get(uint32_t * reg, uint32_t flag);
void tmr_set_one_cycle_mode(uint32_t * tmr, uint32_t enable);
void tmr_set_overflow_request_src(uint32_t * tmr, uint32_t src);
void tmr_set_period_buffer_enable(uint32_t * tmr, uint32_t enable);
void tmr_set_primary_mode(uint32_t * tmr, uint32_t mode);
void tmr_set_ext_clock_mode(uint32_t * tmr, uint32_t enable);
void usb_otg_device_connect(void * udev);
void otg_core_soft_reset(void * udev);
void usb_otg_diep0_reset(void * udev);
void usb_otg_ep0_out_setup(void * udev);
void usb_otg_ep_unstall_hw(void * udev, uint32_t ep_addr);
void usb_otg_ep_in_int_clear(void * udev, uint32_t ep_num, uint32_t flags);
uint32_t usb_otg_get_in_ep_status(void * udev, uint32_t ep_num);
void usb_otg_ep_out_int_clear(void * udev, uint32_t ep_num, uint32_t flags);
uint32_t usb_otg_get_out_ep_status(void * udev, uint32_t ep_num);
void usb_otg_ep_deactivate(void * udev, uint32_t ep_addr);
void usb_otg_rx_fifo_flush(void * udev);
void usb_otg_tx_fifo_flush(void * udev, uint32_t fifo_num);
uint32_t usb_otg_get_in_ep_int(void * udev);
uint32_t usb_otg_get_out_ep_int(void * udev);
void usb_otg_int_mask_set(void * udev, uint32_t mask, uint32_t enable);
uint32_t usb_otg_get_global_int_active(void * udev);
void usb_otg_phy_select_reset(void * udev);
void usb_otg_set_global_int_mask(void * udev, uint32_t mask);
uint32_t usb_otg_base_get(void * udev);
void usb_otg_phy_power_set(void * udev, uint32_t power);
void usb_otg_global_int_disable(void * udev);
void usb_otg_global_int_enable(void * udev);
void usb_otg_resume_phy_clock(void * udev);
void usb_otg_read_fifo(void * udev, void * dst, uint32_t len);
void usb_otg_remote_wakeup_set(void * udev, uint32_t enable);
void usb_otg_set_device_addr(void * udev, uint32_t addr);
void usb_otg_rx_fifo_set(void * udev, uint32_t size);
void usb_otg_tx_fifo_set(void * udev, uint32_t ep_num, uint32_t size);
uint32_t usb_otg_get_enum_speed(void * udev);
void usb_otg_write_fifo(void * udev, uint32_t ep_num, void * src, uint32_t len);
void usb_otg_ep_clear_stall(void * udev, uint32_t ep_addr);
uint32_t usb_device_speed_get(void * udev);
void usb_otg_in_xfer_complete(void * udev, uint32_t ep_num);
void usb_otg_core_init(void * udev);
void usb_otg_out_xfer_complete(void * udev, uint32_t ep_num);
void usb_setup_request_handler(otg_dev_handle_t * udev);
void usb_otg_ctrl_out_recv_start(otg_dev_handle_t * udev);
void usb_ep0_in_xfer_start(otg_dev_handle_t * udev, void * buf, uint32_t len);
void usb_ep0_send_zlp(otg_dev_handle_t * udev);
void usb_ep0_open(otg_dev_handle_t * udev);
void usb_device_soft_reset(void * udev);
void usb_otg_enum_done_handler(void * udev);
void usb_otg_ep_close(void * udev, uint32_t ep_addr);
void usb_otg_ep_activate(void * udev, uint32_t ep_addr, uint32_t ep_type, uint32_t max_pkt);
void usb_otg_ep_out_xfer_start(void * udev, uint32_t ep_num);
void usb_otg_ep_in_xfer_start(otg_dev_handle_t * udev, uint32_t ep_num);
void usb_otg_fifo_config(void * udev);
void usb_device_tx_fifo_flush(void * udev, uint32_t fifo_num);
uint32_t usb_otg_get_ep_xfer_len(void * udev, uint32_t ep_num);
void usb_otg_in_ep_handler(otg_dev_handle_t * udev);
void usb_device_init(void * udev);
void usb_setup_class_request(otg_dev_handle_t * udev);
void usb_otg_core_handler(void * udev);
void usb_otg_out_ep_handler(otg_dev_handle_t * udev);
void usb_otg_remote_wakeup_signal(void * udev);
void usb_otg_reset_handler(void * udev);
void usb_device_set_addr(void * udev);
void usb_otg_ep_set_stall(void * udev, uint32_t ep_addr);
void usb_parse_setup_packet(void * buf, void * setup_pkt);
void encoder_gpio_init(void);
void encoder_quadrature_tick(void);
void encoder_knob_dispatch(void);
uint32_t cpacr_get_fpu_enable(void);

/* ── Patch zone ────────────────────────────────────────────────────────── */

#define PATCH_ZONE_START  0x08025800
#define PATCH_ZONE_END    0x08027fff
#define PATCH_ZONE_SIZE   10240  /* 10 KB */

#endif /* FW_V407_H */

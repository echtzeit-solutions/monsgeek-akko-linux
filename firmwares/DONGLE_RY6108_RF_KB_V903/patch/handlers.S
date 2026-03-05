/*
 * Assembly handler stubs for dongle firmware hooks.
 *
 * battery_ep2_trampoline: inserted at rf_tx_handler's speed gate
 * (0x08006A34).  Calls handle_battery_ep2_send() in C, then resumes
 * rf_tx_handler at 0x08006A38 (instruction after the patched CMP+BNE).
 */

    .syntax unified
    .cpu    cortex-m4
    .thumb

    .global battery_ep2_trampoline
    .thumb_func
battery_ep2_trampoline:
    push    {r0-r3, r12, lr}
    bl      handle_battery_ep2_send
    pop     {r0-r3, r12, lr}
    b.w     0x08006A38          @ resume rf_tx_handler (after speed gate)

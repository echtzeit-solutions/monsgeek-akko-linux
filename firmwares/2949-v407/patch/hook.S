/*
 * PoC firmware patch: intercept vendor command 0xFB and return "PATCHED".
 *
 * Placed at 0x08025800 (real flash) via linker script.
 * Called via B.W trampoline patched into vendor_command_dispatch (0x08013304).
 *
 * The trampoline replaces the original 4-byte prologue:
 *   push {r4,r5,r6,r7,r8,r9,r10,lr}   (bytes: 2d e9 f0 47)
 *
 * If cmd_buf[2] == 0xFB:
 *   Write "PATCHED\0" at cmd_buf[3..10], clear cmd_buf[0], return 0
 * Otherwise:
 *   Jump to original+4 to continue normal dispatch
 */

    .syntax unified
    .cpu    cortex-m4
    .thumb

    .section .text.hook, "ax", %progbits
    .global hook_vendor_dispatch
    .thumb_func
    .type   hook_vendor_dispatch, %function

hook_vendor_dispatch:
    /* 1. Execute the displaced prologue instruction */
    push    {r4, r5, r6, r7, r8, r9, r10, lr}

    /* 2. Load cmd buffer pointer from RAM.
     * The original code does: ldr r4, [literal_pool] where literal_pool
     * contains 0x200082DC. At that RAM address is a pointer to the buffer. */
    ldr     r4, =0x200082DC     /* address of the pointer */
    ldr     r4, [r4]            /* r4 = cmd_buf (dereference) */

    /* 3. Check if there's a pending command (cmd_buf[0] != 0) */
    ldrb    r0, [r4, #0]
    cbz     r0, .Lno_cmd

    /* 4. Check if it's our custom command (cmd_buf[2] == 0xFB) */
    ldrb    r0, [r4, #2]
    cmp     r0, #0xFB
    bne     .Lnot_ours

    /* 5. It's ours! Write "PATCHED\0" response at cmd_buf[3..10] */
    adr     r1, .Lpatched_str
    ldrb    r0, [r1, #0]        /* 'P' */
    strb    r0, [r4, #3]
    ldrb    r0, [r1, #1]        /* 'A' */
    strb    r0, [r4, #4]
    ldrb    r0, [r1, #2]        /* 'T' */
    strb    r0, [r4, #5]
    ldrb    r0, [r1, #3]        /* 'C' */
    strb    r0, [r4, #6]
    ldrb    r0, [r1, #4]        /* 'H' */
    strb    r0, [r4, #7]
    ldrb    r0, [r1, #5]        /* 'E' */
    strb    r0, [r4, #8]
    ldrb    r0, [r1, #6]        /* 'D' */
    strb    r0, [r4, #9]
    movs    r0, #0
    strb    r0, [r4, #10]       /* NUL terminator */

    /* 6. Clear cmd_buf[0] to mark buffer consumed (same as original epilogue) */
    strb    r0, [r4, #0]

    /* 7. Return NULL (r0 already 0) */
    pop     {r4, r5, r6, r7, r8, r9, r10, pc}

.Lno_cmd:
    /* No command pending — return NULL */
    movs    r0, #0
    pop     {r4, r5, r6, r7, r8, r9, r10, pc}

.Lnot_ours:
    /* Not our command — jump to original function after the push.
     * Real address = 0x08013308, with thumb bit = 0x08013309. */
    ldr     r0, =0x08013309
    bx      r0

    .align 2
.Lpatched_str:
    .ascii  "PATCHED\0"

    .size   hook_vendor_dispatch, . - hook_vendor_dispatch

/*
 * Assembly handler stubs for firmware hooks.
 *
 * Each handler is called by the auto-generated stub in hooks_gen.S.
 * Convention for "filter" mode handlers:
 *   - Called with all original registers intact (saved/restored by stub)
 *   - Return r0 = 0: passthrough to original function
 *   - Return r0 != 0: intercept (stub returns to caller, original never runs)
 *   - May freely clobber r0-r3, r12 (caller-saved per AAPCS)
 *
 * All handlers are now in handlers.c (C implementation).
 * This file is kept for any future ASM-only handlers.
 */

    .syntax unified
    .cpu    cortex-m4
    .thumb

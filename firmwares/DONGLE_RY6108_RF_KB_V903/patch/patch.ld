/* Dongle patch linker script.
 * Patch zone: free flash after firmware code, before config region.
 * SRAM: above highest dongle usage (SP tops at ~0x20001088). */
MEMORY {
    PATCH (rx)      : ORIGIN = 0x0800B000, LENGTH = 10240
    PATCH_SRAM (rw) : ORIGIN = 0x20002000, LENGTH = 1024
}
SECTIONS {
    .text : {
        *(.text*)
    } > PATCH
    .rodata : {
        *(.rodata*)
    } > PATCH
    .bss (NOLOAD) : {
        *(.bss*)
        *(COMMON)
    } > PATCH_SRAM
    /DISCARD/ : {
        *(.ARM.*)
        *(.comment)
        *(.note*)
        *(.data*)
    }
}

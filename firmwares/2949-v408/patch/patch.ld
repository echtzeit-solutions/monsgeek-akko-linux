/* Auto-generated from Ghidra project 'firmware_reconstructed.bin'. Do not edit manually.
 * Patch zone: gap between firmware code and config region.
 * Aligned to 2048-byte sector boundary (AT32F405 erase granularity). */
MEMORY {
    PATCH (rx)      : ORIGIN = 0x08025800, LENGTH = 10240
    PATCH_SRAM (rw) : ORIGIN = 0x20009800, LENGTH = 4096
}
SECTIONS {
    .text : {
        *(.text*)
    } > PATCH
    .rodata : {
        *(.rodata*)
    } > PATCH
    .bss (NOLOAD) : {
        __patch_bss_start = .;
        *(.rtt*)
        *(.bss*)
        *(COMMON)
        __patch_bss_end = .;
    } > PATCH_SRAM
    /DISCARD/ : {
        *(.ARM.*)
        *(.comment)
        *(.note*)
        *(.data*)
    }
}

/* Linker script for SRAM shellcode.
 * Code runs from the cmd_buf staging area at 0x20008320.
 * The stack is inherited from flash_save_userpic (valid, ~2KB deep). */
MEMORY {
    SRAM (rwx) : ORIGIN = 0x20008320, LENGTH = 256
}
ENTRY(shellcode_entry)
SECTIONS {
    .text : {
        *(.text.entry)
        *(.text*)
        *(.rodata*)
    } > SRAM
    /DISCARD/ : {
        *(.ARM.*)
        *(.comment)
        *(.note*)
    }
}

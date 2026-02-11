PREFIX  := arm-none-eabi-
CC      := $(PREFIX)gcc
AS      := $(PREFIX)gcc
LD      := $(PREFIX)ld
OBJCOPY := $(PREFIX)objcopy
OBJDUMP := $(PREFIX)objdump

CFLAGS  := -mcpu=cortex-m4 -mthumb -mfloat-abi=hard -mfpu=fpv4-sp-d16 \
           -Os -nostdlib -ffreestanding -Wall
ASFLAGS := -mcpu=cortex-m4 -mthumb -mfloat-abi=hard -mfpu=fpv4-sp-d16 \
           -nostdlib -ffreestanding

LDSCRIPT := patch.ld
FIRMWARE := ../firmware_reconstructed.bin

# Ghidra symbol export / code generation
GHIDRA      ?= /opt/ghidra/support/analyzeHeadless
PROJECT_DIR := ..
PROJECT     := ghidra_v407
PROGRAM     := firmware_reconstructed.bin
SYMBOLS     := symbols.json
SCRIPTS     := ../../../ghidra_scripts
GENERATE    := ../../../scripts/generate_patch_files.py

# ── Framework-based build (hooks_gen.S + handlers.S + handlers.c) ────────
HOOK_OBJS := hooks_gen.o handlers.o handlers_c.o

.PHONY: all clean patch disasm symbols generate shellcode

all: hook.bin

# Generate stubs from hook definitions
hooks_gen.S: hooks.py hook_framework.py $(FIRMWARE)
	python3 hooks.py generate

%.o: %.S
	$(AS) $(ASFLAGS) -c $< -o $@

# C handlers — compiled separately to avoid name collision with handlers.S
handlers_c.o: handlers.c fw_v407.h
	$(CC) $(CFLAGS) -c handlers.c -o $@

hook.elf: $(HOOK_OBJS) $(LDSCRIPT) fw_symbols.ld
	$(LD) -T $(LDSCRIPT) -T fw_symbols.ld $(HOOK_OBJS) -o $@

hook.bin: hook.elf
	$(OBJCOPY) -O binary $< $@

disasm: hook.elf
	$(OBJDUMP) -d $<

# Apply trampolines to firmware
patch: hook.bin $(FIRMWARE)
	python3 hooks.py patch

symbols: $(SYMBOLS)
$(SYMBOLS): $(PROJECT_DIR)/$(PROJECT).rep
	$(GHIDRA) $(PROJECT_DIR) $(PROJECT) -process $(PROGRAM) \
	  -noanalysis -scriptPath $(abspath $(SCRIPTS)) \
	  -postScript ExportSymbols.java $(abspath $@)

generate: $(SYMBOLS)
	python3 $(GENERATE) $(SYMBOLS) -o . --header-name fw_v407.h

# ── SRAM shellcode (for userpic overflow PoC) ────────────────────────────
shellcode: shellcode_led.bin

shellcode_led.o: shellcode_led.c fw_v407_macro.h
	$(CC) $(CFLAGS) -c $< -o $@

shellcode_led.elf: shellcode_led.o shellcode.ld
	$(LD) -T shellcode.ld $< -o $@

shellcode_led.bin: shellcode_led.elf
	$(OBJCOPY) -O binary $< $@
	@echo "Shellcode: $$(wc -c < $@) bytes"

shellcode-disasm: shellcode_led.elf
	$(OBJDUMP) -d $<

clean:
	rm -f $(HOOK_OBJS) hook.elf hook.bin hooks_gen.S ../firmware_patched.bin
	rm -f shellcode_led.o shellcode_led.elf shellcode_led.bin

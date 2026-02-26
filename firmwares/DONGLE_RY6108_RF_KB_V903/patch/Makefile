PREFIX  := arm-none-eabi-
CC      := $(PREFIX)gcc
AS      := $(PREFIX)gcc
LD      := $(PREFIX)ld
OBJCOPY := $(PREFIX)objcopy
OBJDUMP := $(PREFIX)objdump

SHARED  := ../../../patch
SDK     := $(SHARED)/at32f405_sdk
CFLAGS  := -mcpu=cortex-m4 -mthumb -mfloat-abi=hard -mfpu=fpv4-sp-d16 \
           -Os -nostdlib -ffreestanding -Wall \
           -I$(SHARED) \
           -I$(SDK)/cmsis -I$(SDK)/drivers -I$(SDK)/usbotg_library \
           -DAT32F405KBU7_4 -DUSE_STDPERIPH_DRIVER -DUSE_OTG_DEVICE_MODE
ASFLAGS := -mcpu=cortex-m4 -mthumb -mfloat-abi=hard -mfpu=fpv4-sp-d16 \
           -nostdlib -ffreestanding

LDSCRIPT := patch.ld
FIRMWARE := ../dfu_dumps/dongle_working_256k.bin

# ── Framework-based build (hooks_gen.S + handlers.S + handlers.c) ────────
HOOK_OBJS := hooks_gen.o handlers.o handlers_c.o

.PHONY: all clean patch disasm

all: hook.bin

# Generate stubs from hook definitions
hooks_gen.S: hooks.py $(SHARED)/hook_framework.py $(FIRMWARE)
	python3 hooks.py generate

%.o: %.S
	$(AS) $(ASFLAGS) -c $< -o $@

# C handlers
handlers_c.o: handlers.c fw_dongle.h
	$(CC) $(CFLAGS) -c handlers.c -o $@

hook.elf: $(HOOK_OBJS) $(LDSCRIPT) fw_symbols.ld
	$(LD) -T $(LDSCRIPT) -T fw_symbols.ld $(HOOK_OBJS) -o $@

hook.bin: hook.elf
	$(OBJCOPY) -O binary $< $@

disasm: hook.elf
	$(OBJDUMP) -d $<

# Apply trampolines + binary patches to firmware
patch: hook.bin $(FIRMWARE)
	python3 hooks.py patch

clean:
	rm -f $(HOOK_OBJS) hook.elf hook.bin hooks_gen.S
	rm -f ../dfu_dumps/dongle_patched_256k.bin

// Label interrupt vector table entries as functions with proper ISR names.
// Reads the vector table at VTOR (0x08005200), resolves each entry to a function,
// and renames it (e.g., SysTick_Handler, TMR4_IRQHandler).
//
// Safe to run multiple times — skips functions already labeled by the user.
//
// @category Export

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.mem.*;
import ghidra.program.model.symbol.*;

public class LabelVectorTable extends GhidraScript {

    @Override
    public void run() throws Exception {
        Address vtorAddr = toAddr(0x08005200);
        Memory mem = currentProgram.getMemory();
        FunctionManager fm = currentProgram.getFunctionManager();
        SymbolTable st = currentProgram.getSymbolTable();

        // Cortex-M4 system exception names (indices 1-15, index 0 is SP)
        String[] sysNames = {
            null,              // 0: Initial SP
            "Reset_Handler",   // 1
            "NMI_Handler",     // 2
            "HardFault_Handler", // 3
            "MemManage_Handler", // 4
            "BusFault_Handler",  // 5
            "UsageFault_Handler", // 6
            null, null, null, null, // 7-10: reserved
            "SVCall_Handler",    // 11
            "DebugMon_Handler",  // 12
            null,                // 13: reserved
            "PendSV_Handler",    // 14
            "SysTick_Handler",   // 15
        };

        // AT32F405 device IRQ names (index 16+ = IRQ 0+)
        String[] irqNames = {
            "WWDT_IRQHandler",           // IRQ 0
            "PVM_IRQHandler",            // IRQ 1
            "TAMP_STAMP_IRQHandler",     // IRQ 2
            "ERTC_WKUP_IRQHandler",      // IRQ 3
            "FLASH_IRQHandler",          // IRQ 4
            "CRM_IRQHandler",            // IRQ 5
            "EXINT0_IRQHandler",         // IRQ 6
            "EXINT1_IRQHandler",         // IRQ 7
            "EXINT2_IRQHandler",         // IRQ 8
            "EXINT3_IRQHandler",         // IRQ 9
            "EXINT4_IRQHandler",         // IRQ 10
            "DMA1_CH1_IRQHandler",       // IRQ 11
            "DMA1_CH2_IRQHandler",       // IRQ 12
            "DMA1_CH3_IRQHandler",       // IRQ 13
            "DMA1_CH4_IRQHandler",       // IRQ 14
            "DMA1_CH5_IRQHandler",       // IRQ 15
            "DMA1_CH6_IRQHandler",       // IRQ 16
            "DMA1_CH7_IRQHandler",       // IRQ 17
            "ADC1_IRQHandler",           // IRQ 18
            null,                        // IRQ 19
            null,                        // IRQ 20
            null,                        // IRQ 21
            null,                        // IRQ 22
            "EXINT9_5_IRQHandler",       // IRQ 23
            "TMR1_BRK_TMR9_IRQHandler",  // IRQ 24
            "TMR1_OV_TMR10_IRQHandler",  // IRQ 25
            "TMR1_TRG_HALL_TMR11_IRQHandler", // IRQ 26
            "TMR1_CH_IRQHandler",        // IRQ 27
            "TMR2_IRQHandler",           // IRQ 28
            "TMR3_IRQHandler",           // IRQ 29
            "TMR4_IRQHandler",           // IRQ 30
            "I2C1_EVT_IRQHandler",       // IRQ 31
            "I2C1_ERR_IRQHandler",       // IRQ 32
            "I2C2_EVT_IRQHandler",       // IRQ 33
            "I2C2_ERR_IRQHandler",       // IRQ 34
            "SPI1_IRQHandler",           // IRQ 35
            "SPI2_IRQHandler",           // IRQ 36
            "USART1_IRQHandler",         // IRQ 37
            "USART2_IRQHandler",         // IRQ 38
            "USART3_IRQHandler",         // IRQ 39
            "EXINT15_10_IRQHandler",     // IRQ 40
            "ERTCAlarm_IRQHandler",      // IRQ 41
            "OTGFS1_WKUP_IRQHandler",    // IRQ 42
            null, null, null, null, null, // IRQ 43-47
            null, null, null, null,      // IRQ 48-51
            "DMA2_CH1_IRQHandler",       // IRQ 52
            "DMA2_CH2_IRQHandler",       // IRQ 53
            "DMA2_CH3_IRQHandler",       // IRQ 54
            "DMA2_CH4_IRQHandler",       // IRQ 55
            "DMA2_CH5_IRQHandler",       // IRQ 56
            null, null, null,            // IRQ 57-59
            "OTGFS1_IRQHandler",         // IRQ 60
            null, null, null, null, null, // IRQ 61-65
            null, null, null, null, null, // IRQ 66-70
            null, null,                  // IRQ 71-72
            "DMA2_CH6_IRQHandler",       // IRQ 73
            "DMA2_CH7_IRQHandler",       // IRQ 74
            null, null, null, null,      // IRQ 75-78
            "OTGFS2_IRQHandler",         // IRQ 79
        };

        int totalEntries = 16 + irqNames.length;
        int labeled = 0;
        int skipped = 0;
        int created = 0;
        Address defaultHandler = null;

        for (int i = 1; i < totalEntries; i++) {
            String name;
            if (i < sysNames.length) {
                name = sysNames[i];
            } else {
                int irqIdx = i - 16;
                name = (irqIdx >= 0 && irqIdx < irqNames.length) ? irqNames[irqIdx] : null;
            }
            if (name == null) continue;

            Address entryAddr = vtorAddr.add(i * 4);
            int raw = mem.getInt(entryAddr);
            if (raw == 0) continue;

            Address funcAddr = toAddr(raw & ~1); // Clear Thumb bit

            // Skip addresses outside flash
            if (funcAddr.getOffset() < 0x08005000 || funcAddr.getOffset() > 0x08030000) continue;

            // Track the default handler (most common stub)
            Function fn = fm.getFunctionAt(funcAddr);

            // Check if this is the default stub (shared by many vectors)
            // We'll label the first one we find as Default_Handler
            if (defaultHandler == null) {
                // Count how many vectors point here
                int refCount = 0;
                for (int j = 1; j < totalEntries; j++) {
                    Address a = vtorAddr.add(j * 4);
                    int r = mem.getInt(a);
                    if ((r & ~1) == (raw & ~1)) refCount++;
                }
                if (refCount > 10) {
                    defaultHandler = funcAddr;
                }
            }

            if (funcAddr.equals(defaultHandler)) {
                // Label the default handler once
                if (fn != null && isAutoName(fn.getName())) {
                    fn.setName("Default_Handler", SourceType.USER_DEFINED);
                    println("  Labeled default handler: " + funcAddr);
                }
                continue; // Don't give it individual ISR names
            }

            if (fn == null) {
                // Create function if it doesn't exist
                fn = fm.createFunction(null, funcAddr,
                    new AddressSet(funcAddr, funcAddr), SourceType.DEFAULT);
                if (fn == null) {
                    println("  WARN: Could not create function at " + funcAddr + " for " + name);
                    continue;
                }
                created++;
            }

            // Only rename if it's an auto-generated name
            if (isAutoName(fn.getName())) {
                fn.setName(name, SourceType.USER_DEFINED);
                println("  Labeled: " + funcAddr + " -> " + name);
                labeled++;
            } else {
                // Already has a user-defined name — don't overwrite
                // But add the ISR name as a secondary label if different
                if (!fn.getName().equals(name)) {
                    println("  Keep: " + funcAddr + " = " + fn.getName() +
                            " (vector: " + name + ")");
                    // Add as a label (not primary)
                    st.createLabel(funcAddr, name, SourceType.USER_DEFINED);
                }
                skipped++;
            }
        }

        println("\nVector table labeling complete:");
        println("  " + labeled + " functions labeled");
        println("  " + skipped + " already had user-defined names");
        println("  " + created + " new functions created");
        if (defaultHandler != null) {
            println("  Default handler at " + defaultHandler);
        }
    }

    private boolean isAutoName(String name) {
        return name.startsWith("FUN_") ||
               name.startsWith("DAT_") ||
               name.startsWith("LAB_") ||
               name.startsWith("thunk_FUN_") ||
               name.startsWith("switchD_") ||
               name.startsWith("caseD_") ||
               name.startsWith("SUB_") ||
               name.startsWith("EXT_");
    }
}

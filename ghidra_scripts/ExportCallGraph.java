// Export call graph edges for user-defined functions to JSON.
// Output: array of {"caller": "name", "callee": "name"} edges.
// Only includes edges where both endpoints are user-defined functions.
//
// Usage (headless):
//   analyzeHeadless /path/to/project project_name \
//     -process program_name -noanalysis \
//     -scriptPath /path/to/ghidra_scripts \
//     -postScript ExportCallGraph.java /path/to/output.json
//
// @category Export

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;

import java.io.*;
import java.util.*;

public class ExportCallGraph extends GhidraScript {

    private StringBuilder jb;
    private int jDepth;
    private boolean jNeedComma;

    private void jInit() { jb = new StringBuilder(); jDepth = 0; jNeedComma = false; }
    private void jNl() { jb.append('\n'); for (int i = 0; i < jDepth; i++) jb.append("  "); }
    private void jComma() { if (jNeedComma) jb.append(','); }
    private void jEsc(String s) {
        jb.append('"');
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (c == '"') jb.append("\\\"");
            else if (c == '\\') jb.append("\\\\");
            else jb.append(c);
        }
        jb.append('"');
    }
    private void jObjOpen()  { jComma(); jNl(); jb.append('{'); jDepth++; jNeedComma = false; }
    private void jObjClose() { jDepth--; jNl(); jb.append('}'); jNeedComma = true; }
    private void jArrOpen()  { jComma(); jNl(); jb.append('['); jDepth++; jNeedComma = false; }
    private void jArrClose() { jDepth--; jNl(); jb.append(']'); jNeedComma = true; }
    private void jKey(String k) { jComma(); jNl(); jEsc(k); jb.append(": "); jNeedComma = false; }
    private void jKvStr(String k, String v) { jKey(k); jEsc(v); jNeedComma = true; }
    private void jKvBool(String k, boolean v) { jKey(k); jb.append(v); jNeedComma = true; }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String outputPath;
        if (args.length > 0) {
            outputPath = args[0];
        } else {
            outputPath = askString("Output path", "JSON output file path:");
        }

        FunctionManager fm = currentProgram.getFunctionManager();

        // Build set of all function addresses
        Set<Address> funcAddrs = new HashSet<>();
        Map<Address, String> addrToName = new HashMap<>();
        Set<Address> userDefined = new HashSet<>();
        FunctionIterator allFuncs = fm.getFunctions(true);
        while (allFuncs.hasNext()) {
            Function f = allFuncs.next();
            // Skip thunks — they're just trampolines, not real functions
            if (f.isThunk()) continue;
            funcAddrs.add(f.getEntryPoint());
            addrToName.put(f.getEntryPoint(), f.getName());
            if (f.getSymbol().getSource() == SourceType.USER_DEFINED && !isAutoName(f.getName())) {
                userDefined.add(f.getEntryPoint());
            }
        }
        println("Total functions: " + funcAddrs.size() +
                " (" + userDefined.size() + " user-defined)");

        // Export which ones are labeled, so renderer can style them differently
        jInit();
        jb.append("{");
        jDepth++;
        jNeedComma = false;

        jKey("nodes");
        jArrOpen();
        for (Address addr : funcAddrs) {
            jObjOpen();
            jKvStr("name", addrToName.get(addr));
            jKvStr("addr", "0x" + addr.toString());
            jKvBool("labeled", userDefined.contains(addr));
            jObjClose();
        }
        jArrClose();

        // Collect edges
        jKey("edges");
        jArrOpen();
        int edgeCount = 0;

        for (Address callerAddr : funcAddrs) {
            Function caller = fm.getFunctionAt(callerAddr);
            if (caller == null) continue;
            String callerName = caller.getName();

            Set<Function> callees = caller.getCalledFunctions(monitor);
            for (Function callee : callees) {
                if (callee.isThunk()) {
                    // Follow thunk to its target
                    callee = callee.getThunkedFunction(true);
                    if (callee == null) continue;
                }
                Address calleeAddr = callee.getEntryPoint();
                if (funcAddrs.contains(calleeAddr)) {
                    jObjOpen();
                    jKvStr("caller", callerName);
                    jKvStr("callee", addrToName.get(calleeAddr));
                    jObjClose();
                    edgeCount++;
                }
            }
        }

        jArrClose();

        jDepth--;
        jNl();
        jb.append('}');

        println("Exported " + edgeCount + " call edges");

        File outFile = new File(outputPath);
        try (Writer fw = new FileWriter(outFile)) {
            fw.write(jb.toString());
            fw.write('\n');
        }
        println("Wrote " + outFile.getAbsolutePath());
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

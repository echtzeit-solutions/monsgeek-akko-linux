// Export user-defined symbols from the current program to JSON.
// Exports: program metadata, memory blocks, functions, labels, structs, enums.
// Filters out auto-generated names (FUN_*, DAT_*, LAB_*, thunk_*, etc.)
//
// Usage (headless):
//   analyzeHeadless /path/to/project project_name \
//     -process program_name -noanalysis \
//     -scriptPath /path/to/ghidra_scripts \
//     -postScript ExportSymbols.java /path/to/output.json
//
// Usage (GUI): Run from Script Manager, enter output path when prompted.
//
// @category Export

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.data.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.mem.*;
import ghidra.program.model.symbol.*;

import java.io.*;
import java.util.*;

public class ExportSymbols extends GhidraScript {

    // Inline JSON writer state — avoids inner classes (Ghidra OSGi can't handle them)
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
            switch (c) {
                case '"':  jb.append("\\\""); break;
                case '\\': jb.append("\\\\"); break;
                case '\n': jb.append("\\n");  break;
                case '\r': jb.append("\\r");  break;
                case '\t': jb.append("\\t");  break;
                default:   jb.append(c);
            }
        }
        jb.append('"');
    }
    private void jObjOpen()  { jComma(); jNl(); jb.append('{'); jDepth++; jNeedComma = false; }
    private void jObjClose() { jDepth--; jNl(); jb.append('}'); jNeedComma = true; }
    private void jArrOpen()  { jComma(); jNl(); jb.append('['); jDepth++; jNeedComma = false; }
    private void jArrClose() { jDepth--; jNl(); jb.append(']'); jNeedComma = true; }
    private void jKey(String k) { jComma(); jNl(); jEsc(k); jb.append(": "); jNeedComma = false; }
    private void jKvStr(String k, String v) { jKey(k); jEsc(v); jNeedComma = true; }
    private void jKvNum(String k, long v)   { jKey(k); jb.append(v); jNeedComma = true; }
    private void jKvBool(String k, boolean v){ jKey(k); jb.append(v); jNeedComma = true; }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String outputPath;
        if (args.length > 0) {
            outputPath = args[0];
        } else {
            outputPath = askString("Output path", "JSON output file path:");
        }

        jInit();
        jObjOpen();

        jKey("program");
        writeProgramInfo();

        jKey("memory_blocks");
        writeMemoryBlocks();

        jKey("functions");
        writeFunctions();

        jKey("labels");
        writeLabels();

        jKey("structs");
        writeStructs();

        jKey("enums");
        writeEnums();

        jObjClose();

        File outFile = new File(outputPath);
        try (Writer fw = new FileWriter(outFile)) {
            fw.write(jb.toString());
            fw.write('\n');
        }
        println("Exported symbols to " + outFile.getAbsolutePath());
    }

    private void writeProgramInfo() {
        Address base = currentProgram.getImageBase();
        jObjOpen();
        jKvStr("name", currentProgram.getName());
        jKvStr("image_base", "0x" + base.toString());
        jKvStr("arch", currentProgram.getLanguageID().toString());
        jKvStr("endian", currentProgram.getLanguage().isBigEndian() ? "big" : "little");
        jKvNum("pointer_size", currentProgram.getDefaultPointerSize());
        jObjClose();
    }

    private void writeMemoryBlocks() {
        Memory mem = currentProgram.getMemory();
        jArrOpen();
        for (MemoryBlock block : mem.getBlocks()) {
            StringBuilder perms = new StringBuilder();
            if (block.isRead()) perms.append('r');
            if (block.isWrite()) perms.append('w');
            if (block.isExecute()) perms.append('x');

            jObjOpen();
            jKvStr("name", block.getName());
            jKvStr("start", fmtAddr(block.getStart()));
            jKvStr("end", fmtAddr(block.getEnd()));
            jKvNum("size", block.getSize());
            jKvStr("perms", perms.toString());
            jKvBool("initialized", block.isInitialized());
            jObjClose();
        }
        jArrClose();
    }

    private void writeFunctions() {
        FunctionManager fm = currentProgram.getFunctionManager();
        FunctionIterator iter = fm.getFunctions(true);
        int count = 0;
        jArrOpen();
        while (iter.hasNext()) {
            Function func = iter.next();
            if (func.getSymbol().getSource() != SourceType.USER_DEFINED) continue;
            String name = func.getName();
            if (isAutoName(name)) continue;

            jObjOpen();
            jKvStr("name", name);
            jKvStr("addr", fmtAddr(func.getEntryPoint()));
            jKvNum("size", func.getBody().getNumAddresses());
            jKvStr("ret", func.getReturnType().getDisplayName());
            jKvStr("cc", func.getCallingConventionName());

            jKey("params");
            jArrOpen();
            for (Parameter p : func.getParameters()) {
                jObjOpen();
                jKvStr("name", p.getName());
                jKvStr("type", p.getDataType().getDisplayName());
                jKvStr("storage", p.getVariableStorage().toString());
                jObjClose();
            }
            jArrClose();
            jObjClose();
            count++;
        }
        jArrClose();
        println("Exported " + count + " user-defined functions");
    }

    private void writeLabels() {
        SymbolTable st = currentProgram.getSymbolTable();
        FunctionManager fm = currentProgram.getFunctionManager();
        SymbolIterator iter = st.getAllSymbols(true);
        int count = 0;
        jArrOpen();
        while (iter.hasNext()) {
            Symbol sym = iter.next();
            SourceType src = sym.getSource();
            if (src != SourceType.USER_DEFINED && src != SourceType.IMPORTED) continue;
            if (sym.getSymbolType() == SymbolType.FUNCTION) continue;
            if (fm.getFunctionAt(sym.getAddress()) != null) continue;
            String name = sym.getName();
            if (isAutoName(name)) continue;

            jObjOpen();
            jKvStr("name", name);
            jKvStr("addr", fmtAddr(sym.getAddress()));
            jKvBool("primary", sym.isPrimary());

            Data data = getDataAt(sym.getAddress());
            if (data != null) {
                DataType dt = data.getDataType();
                if (dt != null && !dt.getName().startsWith("undefined")) {
                    jKvStr("data_type", dt.getDisplayName());
                    jKvNum("data_size", dt.getLength());
                }
            }
            jObjClose();
            count++;
        }
        jArrClose();
        println("Exported " + count + " user-defined labels");
    }

    private void writeStructs() {
        DataTypeManager dtm = currentProgram.getDataTypeManager();
        Iterator<DataType> iter = dtm.getAllDataTypes();
        int count = 0;
        jArrOpen();
        while (iter.hasNext()) {
            DataType dt = iter.next();
            if (!(dt instanceof Structure)) continue;
            CategoryPath cat = dt.getCategoryPath();
            if (isBuiltinCategory(cat)) continue;

            Structure s = (Structure) dt;
            jObjOpen();
            jKvStr("name", s.getName());
            jKvNum("size", s.getLength());
            jKvStr("category", cat.toString());

            jKey("fields");
            jArrOpen();
            for (DataTypeComponent comp : s.getDefinedComponents()) {
                jObjOpen();
                jKvStr("name", comp.getFieldName() != null ? comp.getFieldName() : "");
                jKvNum("offset", comp.getOffset());
                jKvStr("type", comp.getDataType().getDisplayName());
                jKvNum("size", comp.getLength());
                jObjClose();
            }
            jArrClose();
            jObjClose();
            count++;
        }
        jArrClose();
        println("Exported " + count + " user-defined structs");
    }

    private void writeEnums() {
        DataTypeManager dtm = currentProgram.getDataTypeManager();
        Iterator<DataType> iter = dtm.getAllDataTypes();
        int count = 0;
        jArrOpen();
        while (iter.hasNext()) {
            DataType dt = iter.next();
            if (!(dt instanceof ghidra.program.model.data.Enum)) continue;
            CategoryPath cat = dt.getCategoryPath();
            if (isBuiltinCategory(cat)) continue;

            ghidra.program.model.data.Enum e = (ghidra.program.model.data.Enum) dt;
            jObjOpen();
            jKvStr("name", e.getName());
            jKvNum("size", e.getLength());
            jKvStr("category", cat.toString());

            jKey("members");
            jArrOpen();
            for (String name : e.getNames()) {
                jObjOpen();
                jKvStr("name", name);
                jKvNum("value", e.getValue(name));
                jObjClose();
            }
            jArrClose();
            jObjClose();
            count++;
        }
        jArrClose();
        println("Exported " + count + " user-defined enums");
    }

    // --- Helpers ---

    private String fmtAddr(Address addr) {
        return "0x" + addr.toString();
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

    private boolean isBuiltinCategory(CategoryPath cat) {
        String path = cat.toString();
        return path.startsWith("/BuiltInTypes") ||
               path.startsWith("/CodeBrowser") ||
               path.startsWith("/pointer");
    }
}

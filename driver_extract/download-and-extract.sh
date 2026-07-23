#!/bin/bash
# MonsGeek/Epomaker/Akko/Womier IOT Driver — Download & Extraction Pipeline
#
# Unpacks a vendor Electron driver into a per-vendor workspace under vendors/<tag>/,
# ready for scripts/update-device-db.sh to mine for devices, key matrices and layouts.
#
# The driver can come from the built-in Akko Cloud URLs or from any locally downloaded
# installer (--input). Vendors that rebrand the Akko app ship the same data for their own
# models plus everything else in that build, so a local vendor driver is often the only
# source for a recently released keyboard.
#
# Two bundle formats are handled automatically:
#   monolithic  one dist/index.*.js — needs webcrack + the Babel refactor to split apart
#   chunks      Vite code-split dist/js/*.js, one chunk per device class — used directly
#
# Usage:
#   ./download-and-extract.sh [--version v3|v4] [--browser] [--clean]
#   ./download-and-extract.sh --input <installer.rar|.zip|.exe|.7z|dir> --vendor <tag>

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Driver download URLs
DRIVER_V4_URL="https://file.akkogear.com/Akko_Cloud_v4_setup_370.2.17_WIN20251225.zip"
DRIVER_V3_URL="https://file.akkogear.com/Akko_Cloud_setup_370.1.54(WIN2025073).zip"

# A code-split dist/js holds one chunk per device class; a monolithic build has a handful
# of files at most. Well clear of both.
CHUNK_COUNT_THRESHOLD=50

DRIVER_VERSION="v4"
VENDOR=""
INPUT=""
SETUP_BROWSER=false
CLEAN_BUILD=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --version)
            DRIVER_VERSION="$2"
            shift 2
            ;;
        --input)
            INPUT="$2"
            shift 2
            ;;
        --vendor)
            VENDOR="$2"
            shift 2
            ;;
        --browser)
            SETUP_BROWSER=true
            shift
            ;;
        --clean)
            CLEAN_BUILD=true
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [options]"
            echo ""
            echo "Options:"
            echo "  --input <path>    Local installer (.rar/.zip/.exe/.7z) or an already-"
            echo "                    unpacked directory, instead of downloading Akko Cloud"
            echo "  --vendor <tag>    Workspace name under vendors/ (default: akko)"
            echo "  --version v3|v4   Akko Cloud version to download (default: v4)"
            echo "  --browser         Set up browser app with Vite after extraction"
            echo "  --clean           Wipe this vendor's workspace first"
            echo "  --help            Show this help"
            echo ""
            echo "Examples:"
            echo "  $0                                              # Akko Cloud v4"
            echo "  $0 --input ~/Downloads/Womier_SK75.rar --vendor womier"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

[ -z "$VENDOR" ] && VENDOR="akko"

if [ "$DRIVER_VERSION" = "v4" ]; then
    DRIVER_URL="$DRIVER_V4_URL"
else
    DRIVER_URL="$DRIVER_V3_URL"
fi

WORKSPACE="$SCRIPT_DIR/vendors/$VENDOR"
UNPACKED_DIR="$WORKSPACE/unpacked"
UNBUNDLED_DIR="$WORKSPACE/unbundled"
REFACTORED_DIR="$WORKSPACE/refactored"
MANIFEST="$WORKSPACE/manifest.json"

echo "=============================================="
echo "IOT Driver Extraction Pipeline"
echo "=============================================="
echo "Vendor:    $VENDOR"
echo "Workspace: $WORKSPACE"
if [ -n "$INPUT" ]; then
    echo "Input:     $INPUT (local)"
else
    echo "Input:     $DRIVER_URL"
fi
echo ""

if [ "$CLEAN_BUILD" = true ]; then
    echo "=== Cleaning $WORKSPACE ==="
    rm -rf "$WORKSPACE"
    echo ""
fi

mkdir -p "$WORKSPACE"

# =============================================================================
# Dependencies
# =============================================================================

check_deps() {
    echo "=== Checking dependencies ==="

    for tool in node npm 7z; do
        if ! command -v "$tool" &> /dev/null; then
            echo "Error: $tool is required"
            [ "$tool" = "7z" ] && echo "Install with: sudo apt install p7zip-full"
            exit 1
        fi
    done
    echo "✓ node $(node --version), npm $(npm --version), 7z"

    if [ -z "$INPUT" ] && ! command -v curl &> /dev/null; then
        echo "Error: curl is required to download the driver"
        exit 1
    fi

    # p7zip cannot decompress RAR5 ("Unsupported Method"); unar/unrar can.
    if [[ "$INPUT" == *.rar ]] && ! command -v unar &> /dev/null && ! command -v unrar &> /dev/null; then
        echo "Error: extracting a .rar needs unar or unrar (7z cannot decompress RAR5)"
        echo "Install with: sudo apt install unar"
        exit 1
    fi

    if ! command -v webcrack &> /dev/null; then
        echo "Installing webcrack..."
        npm install -g webcrack
    fi

    if [ ! -d "$SCRIPT_DIR/node_modules/@babel" ]; then
        echo "Installing babel dependencies..."
        (cd "$SCRIPT_DIR" && npm install)
    fi
    echo "✓ webcrack + babel"
    echo ""
}

# =============================================================================
# Step 1: Acquire the installer
# =============================================================================

acquire() {
    echo "=== Step 1: Acquiring driver ==="

    if [ -n "$INPUT" ]; then
        if [ ! -e "$INPUT" ]; then
            echo "Error: --input path does not exist: $INPUT"
            exit 1
        fi
        SOURCE_PATH="$(cd "$(dirname "$INPUT")" && pwd)/$(basename "$INPUT")"
        echo "Using local input: $SOURCE_PATH"
    else
        mkdir -p "$WORKSPACE/downloads"
        SOURCE_PATH="$WORKSPACE/downloads/$(basename "$DRIVER_URL")"
        if [ -f "$SOURCE_PATH" ]; then
            echo "Already downloaded: $SOURCE_PATH"
        else
            echo "Downloading: $DRIVER_URL"
            curl -L -o "$SOURCE_PATH" "$DRIVER_URL"
        fi
    fi

    [ -f "$SOURCE_PATH" ] && echo "Size: $(du -h "$SOURCE_PATH" | cut -f1)"
    echo ""
}

# =============================================================================
# Step 2: Unpack (recursively — installers nest archives several levels deep)
# =============================================================================

# Unpack one archive into a directory, dispatching on extension.
unpack_one() {
    local archive="$1" dest="$2"
    mkdir -p "$dest"
    case "${archive,,}" in
        *.rar)
            if command -v unar &> /dev/null; then
                unar -q -f -o "$dest" "$archive" > /dev/null
            else
                unrar x -inul -o+ "$archive" "$dest/"
            fi
            ;;
        *.zip)
            unzip -qo "$archive" -d "$dest"
            ;;
        *)
            # .exe (NSIS), .7z, and anything else 7z recognises
            7z x -o"$dest" "$archive" -y > /dev/null
            ;;
    esac
}

# The Electron payload is at resources/app/dist; every vendor installer wraps it in
# some chain of rar -> exe -> app*.7z, so keep unpacking nested archives until it shows up.
find_dist() {
    find "$UNPACKED_DIR" -type d -path "*app/dist" 2>/dev/null | head -1
}

unpack() {
    echo "=== Step 2: Unpacking ==="

    if [ -d "$SOURCE_PATH" ]; then
        UNPACKED_DIR="$SOURCE_PATH"
        echo "Input is a directory — using as-is"
    else
        rm -rf "$UNPACKED_DIR"
        unpack_one "$SOURCE_PATH" "$UNPACKED_DIR"

        local depth=0
        while [ -z "$(find_dist)" ] && [ $depth -lt 5 ]; do
            local nested
            nested=$(find "$UNPACKED_DIR" -type f \
                \( -iname "*.7z" -o -iname "*.exe" -o -iname "*.zip" -o -iname "*.rar" \) \
                -size +1M -not -name ".unpacked-*" 2>/dev/null | head -1)
            [ -z "$nested" ] && break
            echo "  Unpacking nested archive: ${nested#$UNPACKED_DIR/}"
            unpack_one "$nested" "$UNPACKED_DIR/$(basename "$nested" | tr -c 'a-zA-Z0-9._-' '_').d"
            # Drop it so the next pass picks a different archive.
            rm -f "$nested"
            depth=$((depth + 1))
        done
    fi

    APP_DIST=$(find_dist)
    if [ -z "$APP_DIST" ]; then
        echo "Error: could not find the Electron app's dist directory under $UNPACKED_DIR"
        echo "Directory structure:"
        find "$UNPACKED_DIR" -maxdepth 4 -type d | head -30
        exit 1
    fi

    echo "Found app dist: $APP_DIST"
    echo ""
}

# =============================================================================
# Step 3: Detect bundle format
# =============================================================================

detect_format() {
    echo "=== Step 3: Detecting bundle format ==="

    CHUNKS_DIR=""
    BUNDLE_FILE=""

    if [ -d "$APP_DIST/js" ]; then
        local n
        n=$(find "$APP_DIST/js" -maxdepth 1 -name "*.js" | wc -l)
        if [ "$n" -ge "$CHUNK_COUNT_THRESHOLD" ]; then
            FORMAT="chunks"
            CHUNKS_DIR="$APP_DIST/js"
            # The entry chunk holds the device arrays and the KeyLayout enum.
            BUNDLE_FILE=$(find "$CHUNKS_DIR" -maxdepth 1 -name "index.*.js" | head -1)
            [ -z "$BUNDLE_FILE" ] && BUNDLE_FILE=$(find "$CHUNKS_DIR" -maxdepth 1 -name "*.js" -printf '%s %p\n' | sort -rn | head -1 | cut -d' ' -f2)
            echo "Format: code-split ($n chunks)"
            echo "Entry bundle: $BUNDLE_FILE"
            echo ""
            return
        fi
    fi

    FORMAT="monolithic"
    BUNDLE_FILE=$(find "$APP_DIST" -maxdepth 2 -name "index.*.js" -type f | head -1)
    [ -z "$BUNDLE_FILE" ] && BUNDLE_FILE=$(find "$APP_DIST" -maxdepth 2 -name "main.*.js" -type f | head -1)
    [ -z "$BUNDLE_FILE" ] && BUNDLE_FILE=$(find "$APP_DIST" -name "*.js" -type f -printf '%s %p\n' | sort -rn | head -1 | cut -d' ' -f2)

    if [ -z "$BUNDLE_FILE" ]; then
        echo "Error: could not find a JS bundle in $APP_DIST"
        exit 1
    fi

    echo "Format: monolithic"
    echo "Bundle: $BUNDLE_FILE ($(du -h "$BUNDLE_FILE" | cut -f1))"
    echo ""
}

# =============================================================================
# Step 4: Deobfuscate + refactor (monolithic builds only)
# =============================================================================

deobfuscate() {
    echo "=== Step 4: Deobfuscating (webcrack) ==="

    rm -rf "$UNBUNDLED_DIR"
    webcrack "$BUNDLE_FILE" -o "$UNBUNDLED_DIR" 2>&1 || {
        echo "Warning: webcrack had issues, continuing anyway..."
    }

    DEOBFUSCATED_FILE="$UNBUNDLED_DIR/deobfuscated.js"
    if [ ! -f "$DEOBFUSCATED_FILE" ]; then
        DEOBFUSCATED_FILE=$(find "$UNBUNDLED_DIR" -name "*.js" -type f -printf '%s %p\n' | sort -rn | head -1 | cut -d' ' -f2)
    fi
    echo "Deobfuscated: $DEOBFUSCATED_FILE ($(du -h "$DEOBFUSCATED_FILE" | cut -f1))"
    echo "Extracted $(find "$UNBUNDLED_DIR" -name "*.js" | wc -l) JS files"
    echo ""

    echo "=== Step 5: Refactoring into components ==="
    rm -rf "$REFACTORED_DIR"
    (cd "$SCRIPT_DIR" && node refactor-transform.js "$DEOBFUSCATED_FILE" "$REFACTORED_DIR")

    echo "=== Step 6: Adding stubs for undefined references ==="
    (cd "$SCRIPT_DIR" && node add-stubs-transform.js "$REFACTORED_DIR/src/devices" 2>/dev/null || true)
    echo ""
}

# =============================================================================
# Manifest — tells update-device-db.sh what this workspace contains
# =============================================================================

write_manifest() {
    node -e '
        const fs = require("fs");
        const [manifest, vendor, format, bundle, chunks, refactored] = process.argv.slice(1);
        const exists = p => p && fs.existsSync(p) ? p : null;
        fs.writeFileSync(manifest, JSON.stringify({
            vendor, format,
            generatedAt: new Date().toISOString(),
            bundle: exists(bundle),
            chunksDir: exists(chunks),
            refactoredDir: exists(refactored),
            svgDir: exists(refactored ? refactored + "/src/assets/svg" : null),
        }, null, 2));
    ' "$MANIFEST" "$VENDOR" "$FORMAT" "$BUNDLE_FILE" "$CHUNKS_DIR" "$REFACTORED_DIR"
}

# =============================================================================
# Optional browser app (monolithic only — it needs the refactored sources)
# =============================================================================

setup_browser() {
    [ "$SETUP_BROWSER" != true ] && return
    if [ "$FORMAT" != "monolithic" ]; then
        echo "Note: --browser needs the refactored sources; not available for code-split builds."
        return
    fi

    echo "=== Setting up browser app ==="
    cd "$REFACTORED_DIR"

    if [ ! -f "package.json" ]; then
        npm init -y > /dev/null
        node -e '
            const pkg = require("./package.json");
            pkg.type = "module";
            pkg.scripts = { dev: "vite", build: "vite build", preview: "vite preview" };
            require("fs").writeFileSync("./package.json", JSON.stringify(pkg, null, 2));
        '
    fi

    npm install --save-dev vite @vitejs/plugin-react react react-dom

    cat > vite.config.js << 'EOF'
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: { port: 3000, open: true },
  resolve: { alias: { 'electron': '/src/stubs/electron.js' } },
});
EOF

    mkdir -p src/stubs
    cat > src/stubs/electron.js << 'EOF'
// Electron API stubs for browser environment
export const ipcRenderer = {
  send: (channel, ...args) => console.log('[IPC Send]', channel, args),
  on: (channel) => console.log('[IPC On]', channel),
  invoke: async (channel, ...args) => {
    console.log('[IPC Invoke]', channel, args);
    return null;
  },
};

export const ipcMain = { on: () => {}, handle: () => {} };
export const app = { getPath: (name) => `/mock/${name}`, getVersion: () => '1.0.0' };
export const shell = { openExternal: (url) => window.open(url, '_blank') };
export const dialog = {
  showOpenDialog: async () => ({ canceled: true, filePaths: [] }),
  showSaveDialog: async () => ({ canceled: true }),
};
export const BrowserWindow = class {
  constructor() {}
  loadURL() {}
};

export default { ipcRenderer, ipcMain, app, shell, dialog, BrowserWindow };
EOF

    echo "Browser app ready: cd $REFACTORED_DIR && npm run dev"
    echo ""
}

print_summary() {
    echo "=============================================="
    echo "EXTRACTION COMPLETE — vendor '$VENDOR' ($FORMAT)"
    echo "=============================================="
    echo "  Workspace: $WORKSPACE"
    echo "  Manifest:  $MANIFEST"
    echo ""
    echo "Next: ./scripts/update-device-db.sh --vendor $VENDOR"
    echo "=============================================="
}

main() {
    check_deps
    acquire
    unpack
    detect_format
    [ "$FORMAT" = "monolithic" ] && deobfuscate
    write_manifest
    setup_browser
    print_summary
}

main "$@"

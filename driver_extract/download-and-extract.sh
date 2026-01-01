#!/bin/bash
# MonsGeek/Epomaker/Akko IOT Driver - Complete Download & Extraction Pipeline
#
# This script:
# 1. Downloads the Akko Cloud Driver from official sources
# 2. Extracts the NSIS installer
# 3. Extracts the bundled JS from the Electron app
# 4. Runs webcrack to unbundle/deobfuscate
# 5. Runs the Babel-based refactorer to split into components
# 6. Adds stubs for undefined references
# 7. Optionally sets up the browser app with Vite
#
# Usage: ./download-and-extract.sh [--version v3|v4] [--browser] [--clean]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Driver download URLs
DRIVER_V4_URL="https://file.akkogear.com/Akko_Cloud_v4_setup_370.2.17_WIN20251225.zip"
DRIVER_V3_URL="https://file.akkogear.com/Akko_Cloud_setup_370.1.54(WIN2025073).zip"

# Default settings
DRIVER_VERSION="v4"
SETUP_BROWSER=false
CLEAN_BUILD=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --version)
            DRIVER_VERSION="$2"
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
            echo "  --version v3|v4   Driver version to download (default: v4)"
            echo "  --browser         Set up browser app with Vite after extraction"
            echo "  --clean           Clean all previous extraction artifacts"
            echo "  --help            Show this help"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Set driver URL based on version
if [ "$DRIVER_VERSION" = "v4" ]; then
    DRIVER_URL="$DRIVER_V4_URL"
else
    DRIVER_URL="$DRIVER_V3_URL"
fi

echo "=============================================="
echo "MonsGeek/Akko IOT Driver Extraction Pipeline"
echo "=============================================="
echo "Script directory: $SCRIPT_DIR"
echo "Driver version: $DRIVER_VERSION"
echo "Driver URL: $DRIVER_URL"
echo "Setup browser: $SETUP_BROWSER"
echo ""

# Clean if requested
if [ "$CLEAN_BUILD" = true ]; then
    echo "=== Cleaning previous artifacts ==="
    rm -rf "$SCRIPT_DIR/downloads"
    rm -rf "$SCRIPT_DIR/extracted"
    rm -rf "$SCRIPT_DIR/unbundled"
    rm -rf "$SCRIPT_DIR/refactored"
    rm -rf "$SCRIPT_DIR/refactored-v2"
    echo "Cleaned."
    echo ""
fi

# Check dependencies
check_deps() {
    echo "=== Checking dependencies ==="

    if ! command -v node &> /dev/null; then
        echo "Error: Node.js is required"
        exit 1
    fi
    echo "✓ Node.js: $(node --version)"

    if ! command -v npm &> /dev/null; then
        echo "Error: npm is required"
        exit 1
    fi
    echo "✓ npm: $(npm --version)"

    if ! command -v 7z &> /dev/null; then
        echo "Error: 7z (p7zip-full) is required for NSIS extraction"
        echo "Install with: sudo apt install p7zip-full"
        exit 1
    fi
    echo "✓ 7z: $(7z | head -2 | tail -1)"

    if ! command -v curl &> /dev/null; then
        echo "Error: curl is required"
        exit 1
    fi
    echo "✓ curl installed"

    # Check/install webcrack
    if ! command -v webcrack &> /dev/null; then
        echo "Installing webcrack..."
        npm install -g webcrack
    fi
    echo "✓ webcrack installed"

    # Check/install babel dependencies
    if [ ! -d "$SCRIPT_DIR/node_modules/@babel" ]; then
        echo "Installing babel dependencies..."
        cd "$SCRIPT_DIR"
        npm install --save-dev @babel/core @babel/parser @babel/traverse @babel/generator @babel/types prettier
    fi
    echo "✓ Babel dependencies installed"
    echo ""
}

# Step 1: Download driver
download_driver() {
    echo "=== Step 1: Downloading driver ==="

    mkdir -p "$SCRIPT_DIR/downloads"
    DOWNLOAD_FILE="$SCRIPT_DIR/downloads/$(basename "$DRIVER_URL")"

    if [ -f "$DOWNLOAD_FILE" ]; then
        echo "Driver already downloaded: $DOWNLOAD_FILE"
    else
        echo "Downloading from: $DRIVER_URL"
        curl -L -o "$DOWNLOAD_FILE" "$DRIVER_URL"
        echo "Downloaded: $DOWNLOAD_FILE"
    fi

    echo "Size: $(du -h "$DOWNLOAD_FILE" | cut -f1)"
    echo ""
}

# Step 2: Extract ZIP
extract_zip() {
    echo "=== Step 2: Extracting ZIP ==="

    EXTRACT_DIR="$SCRIPT_DIR/downloads/unzipped"
    rm -rf "$EXTRACT_DIR"
    mkdir -p "$EXTRACT_DIR"

    unzip -q "$DOWNLOAD_FILE" -d "$EXTRACT_DIR"

    # Find the .exe installer
    INSTALLER_EXE=$(find "$EXTRACT_DIR" -name "*.exe" -type f | head -1)

    if [ -z "$INSTALLER_EXE" ]; then
        echo "Error: Could not find installer .exe in ZIP"
        exit 1
    fi

    echo "Found installer: $INSTALLER_EXE"
    echo ""
}

# Step 3: Extract NSIS installer
extract_nsis() {
    echo "=== Step 3: Extracting NSIS installer ==="

    EXTRACTED_DIR="$SCRIPT_DIR/extracted"
    rm -rf "$EXTRACTED_DIR"
    mkdir -p "$EXTRACTED_DIR"

    # 7z can extract NSIS installers
    7z x -o"$EXTRACTED_DIR" "$INSTALLER_EXE" -y > /dev/null

    echo "Extracted NSIS to: $EXTRACTED_DIR"

    # Check for nested 7z archive (common in Electron NSIS installers)
    NESTED_7Z=$(find "$EXTRACTED_DIR" -name "app*.7z" -type f | head -1)
    if [ -n "$NESTED_7Z" ]; then
        echo "Found nested archive: $NESTED_7Z"
        APP_EXTRACT_DIR="$EXTRACTED_DIR/app"
        mkdir -p "$APP_EXTRACT_DIR"
        7z x -o"$APP_EXTRACT_DIR" "$NESTED_7Z" -y > /dev/null
        echo "Extracted nested app archive"
    fi

    # Find the Electron app resources
    APP_DIST=$(find "$EXTRACTED_DIR" -path "*/app/dist" -type d | head -1)

    if [ -z "$APP_DIST" ]; then
        # Try alternative paths
        APP_DIST=$(find "$EXTRACTED_DIR" -path "*/resources/app/dist" -type d | head -1)
    fi

    if [ -z "$APP_DIST" ]; then
        # Try finding dist directly
        APP_DIST=$(find "$EXTRACTED_DIR" -name "dist" -type d | head -1)
    fi

    if [ -z "$APP_DIST" ]; then
        echo "Error: Could not find Electron app dist directory"
        echo "Directory structure:"
        find "$EXTRACTED_DIR" -type d | head -30
        exit 1
    fi

    echo "Found app dist: $APP_DIST"
    echo ""
}

# Step 4: Find the bundle file
find_bundle() {
    echo "=== Step 4: Finding bundle file ==="

    # Look for index.*.js pattern (Vite/Rollup bundle)
    BUNDLE_FILE=$(find "$APP_DIST" -name "index.*.js" -type f 2>/dev/null | head -1)

    if [ -z "$BUNDLE_FILE" ]; then
        # Try other common patterns
        BUNDLE_FILE=$(find "$APP_DIST" -name "main.*.js" -type f 2>/dev/null | head -1)
    fi

    if [ -z "$BUNDLE_FILE" ]; then
        # Fall back to largest JS file
        BUNDLE_FILE=$(find "$APP_DIST" -name "*.js" -type f -exec ls -S {} + 2>/dev/null | head -1)
    fi

    if [ -z "$BUNDLE_FILE" ]; then
        echo "Error: Could not find bundle file in $APP_DIST"
        exit 1
    fi

    echo "Found bundle: $BUNDLE_FILE"
    echo "Size: $(du -h "$BUNDLE_FILE" | cut -f1)"
    echo ""
}

# Step 5: Run webcrack
run_webcrack() {
    echo "=== Step 5: Running webcrack ==="

    UNBUNDLED_DIR="$SCRIPT_DIR/unbundled"
    rm -rf "$UNBUNDLED_DIR"

    echo "Output: $UNBUNDLED_DIR"
    webcrack "$BUNDLE_FILE" -o "$UNBUNDLED_DIR" 2>&1 || {
        echo "Warning: webcrack had issues, continuing anyway..."
    }

    # Find the main deobfuscated file
    DEOBFUSCATED_FILE="$UNBUNDLED_DIR/deobfuscated.js"
    if [ ! -f "$DEOBFUSCATED_FILE" ]; then
        DEOBFUSCATED_FILE=$(find "$UNBUNDLED_DIR" -name "*.js" -type f -exec ls -S {} + | head -1)
    fi

    if [ -f "$DEOBFUSCATED_FILE" ]; then
        echo "Deobfuscated file: $DEOBFUSCATED_FILE"
        echo "Size: $(du -h "$DEOBFUSCATED_FILE" | cut -f1)"
    fi

    echo "Extracted $(find "$UNBUNDLED_DIR" -name "*.js" | wc -l) JS files"
    echo ""
}

# Step 6: Run babel refactorer
run_refactor() {
    echo "=== Step 6: Running Babel refactorer ==="

    REFACTORED_DIR="$SCRIPT_DIR/refactored-v2"
    rm -rf "$REFACTORED_DIR"

    echo "Input: $DEOBFUSCATED_FILE"
    echo "Output: $REFACTORED_DIR"

    cd "$SCRIPT_DIR"

    if [ -f "$SCRIPT_DIR/refactor-transform.js" ]; then
        node refactor-transform.js "$DEOBFUSCATED_FILE" "$REFACTORED_DIR"
    else
        echo "Warning: refactor-transform.js not found, skipping transform"
    fi

    echo ""
}

# Step 7: Add stubs
add_stubs() {
    echo "=== Step 7: Adding stubs for undefined references ==="

    if [ -f "$SCRIPT_DIR/add-stubs-transform.js" ]; then
        cd "$SCRIPT_DIR"
        node add-stubs-transform.js "$REFACTORED_DIR/src/devices" 2>/dev/null || true
    else
        echo "Warning: add-stubs-transform.js not found, skipping"
    fi

    echo ""
}

# Step 8: Setup browser app (optional)
setup_browser() {
    if [ "$SETUP_BROWSER" != true ]; then
        return
    fi

    echo "=== Step 8: Setting up browser app ==="

    cd "$REFACTORED_DIR"

    # Initialize package.json if needed
    if [ ! -f "package.json" ]; then
        npm init -y
        # Update package.json for ES modules
        node -e "
            const pkg = require('./package.json');
            pkg.type = 'module';
            pkg.scripts = { dev: 'vite', build: 'vite build', preview: 'vite preview' };
            require('fs').writeFileSync('./package.json', JSON.stringify(pkg, null, 2));
        "
    fi

    # Install Vite and dependencies
    npm install --save-dev vite @vitejs/plugin-react react react-dom

    # Create vite.config.js
    cat > vite.config.js << 'EOF'
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: {
    port: 3000,
    open: true,
  },
  resolve: {
    alias: {
      'electron': '/src/stubs/electron.js',
    },
  },
});
EOF

    # Create electron stubs
    mkdir -p src/stubs
    cat > src/stubs/electron.js << 'EOF'
// Electron API stubs for browser environment
export const ipcRenderer = {
  send: (channel, ...args) => console.log('[IPC Send]', channel, args),
  on: (channel, callback) => console.log('[IPC On]', channel),
  invoke: async (channel, ...args) => {
    console.log('[IPC Invoke]', channel, args);
    return null;
  },
};

export const ipcMain = {
  on: () => {},
  handle: () => {},
};

export const app = {
  getPath: (name) => `/mock/${name}`,
  getVersion: () => '1.0.0',
};

export const shell = {
  openExternal: (url) => window.open(url, '_blank'),
};

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

    echo "Browser app setup complete!"
    echo "Run: cd $REFACTORED_DIR && npm run dev"
    echo ""
}

# Summary
print_summary() {
    echo "=============================================="
    echo "EXTRACTION COMPLETE"
    echo "=============================================="
    echo ""
    echo "Output directories:"
    echo "  • Downloads:    $SCRIPT_DIR/downloads/"
    echo "  • Extracted:    $SCRIPT_DIR/extracted/"
    echo "  • Unbundled:    $SCRIPT_DIR/unbundled/"
    echo "  • Refactored:   $SCRIPT_DIR/refactored-v2/"
    echo ""

    if [ -d "$REFACTORED_DIR/src/devices" ]; then
        DEVICE_COUNT=$(find "$REFACTORED_DIR/src/devices" -name "support*.js" | wc -l)
        echo "Device files: $DEVICE_COUNT"
    fi

    if [ "$SETUP_BROWSER" = true ]; then
        echo ""
        echo "Browser app ready! Start with:"
        echo "  cd $REFACTORED_DIR && npm run dev"
    fi

    echo ""
    echo "=============================================="
}

# Main execution
main() {
    check_deps
    download_driver
    extract_zip
    extract_nsis
    find_bundle
    run_webcrack
    run_refactor
    add_stubs
    setup_browser
    print_summary
}

main "$@"

#!/bin/bash
# MonsGeek/Epomaker IOT Driver - Complete Extraction Pipeline
#
# This script:
# 1. Finds and extracts the bundled JS from the Electron app
# 2. Runs webcrack to unbundle/deobfuscate
# 3. Runs the babel-based refactorer to split into components
# 4. Creates a clean, npm-installable project structure
#
# Usage: ./extract-all.sh [input-app-dir] [output-dir]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INPUT_DIR="${1:-$SCRIPT_DIR/extracted/PLUGINSDIR/app/resources/app/dist}"
OUTPUT_BASE="${2:-$SCRIPT_DIR}"

echo "=============================================="
echo "MonsGeek IOT Driver Extraction Pipeline"
echo "=============================================="
echo "Script directory: $SCRIPT_DIR"
echo "Input directory: $INPUT_DIR"
echo "Output base: $OUTPUT_BASE"
echo ""

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

# Step 1: Find the bundled JS file
find_bundle() {
    echo "=== Step 1: Finding bundle file ==="

    # Look for index.*.js pattern (Vite/Rollup bundle)
    BUNDLE_FILE=$(find "$INPUT_DIR" -name "index.*.js" -type f 2>/dev/null | head -1)

    if [ -z "$BUNDLE_FILE" ]; then
        # Try other common patterns
        BUNDLE_FILE=$(find "$INPUT_DIR" -name "main.*.js" -type f 2>/dev/null | head -1)
    fi

    if [ -z "$BUNDLE_FILE" ]; then
        # Fall back to largest JS file
        BUNDLE_FILE=$(find "$INPUT_DIR" -name "*.js" -type f -exec ls -S {} + 2>/dev/null | head -1)
    fi

    if [ -z "$BUNDLE_FILE" ]; then
        echo "Error: Could not find bundle file in $INPUT_DIR"
        exit 1
    fi

    echo "Found bundle: $BUNDLE_FILE"
    echo "Size: $(du -h "$BUNDLE_FILE" | cut -f1)"
    echo ""
}

# Step 2: Run webcrack
run_webcrack() {
    echo "=== Step 2: Running webcrack ==="

    UNBUNDLED_DIR="$OUTPUT_BASE/unbundled"

    # Clean previous output
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

# Step 3: Run babel refactorer
run_refactor() {
    echo "=== Step 3: Running babel refactorer ==="

    REFACTORED_DIR="$OUTPUT_BASE/refactored"

    # Clean previous output
    rm -rf "$REFACTORED_DIR"

    echo "Input: $DEOBFUSCATED_FILE"
    echo "Output: $REFACTORED_DIR"

    cd "$SCRIPT_DIR"
    node refactor-bundle.js "$DEOBFUSCATED_FILE" "$REFACTORED_DIR"

    echo ""
}

# Step 4: Summary
print_summary() {
    echo "=============================================="
    echo "EXTRACTION COMPLETE"
    echo "=============================================="
    echo ""
    echo "Output directories:"
    echo "  • Unbundled (webcrack):  $OUTPUT_BASE/unbundled/"
    echo "  • Refactored (babel):    $OUTPUT_BASE/refactored/"
    echo ""
    echo "Key files:"
    echo "  • Device definitions:    $OUTPUT_BASE/refactored/src/devices/"
    echo "  • HID Protocol classes:  $OUTPUT_BASE/refactored/src/protocol/"
    echo "  • React components:      $OUTPUT_BASE/refactored/src/components/"
    echo "  • SVG assets:            $OUTPUT_BASE/refactored/src/assets/svg/"
    echo "  • Manifest:              $OUTPUT_BASE/refactored/manifest.json"
    echo "  • package.json:          $OUTPUT_BASE/refactored/package.json"
    echo ""
    echo "To use the refactored code:"
    echo "  cd $OUTPUT_BASE/refactored"
    echo "  npm install"
    echo ""

    # Show device summary if available
    if [ -f "$OUTPUT_BASE/refactored/src/devices/devices.json" ]; then
        echo "Devices found:"
        jq -r '.[] | "  • \(.displayName) (\(.vidHex):\(.pidHex)) - \(.type)"' \
            "$OUTPUT_BASE/refactored/src/devices/devices.json" 2>/dev/null | head -20
        DEVICE_COUNT=$(jq '. | length' "$OUTPUT_BASE/refactored/src/devices/devices.json" 2>/dev/null)
        if [ "$DEVICE_COUNT" -gt 20 ]; then
            echo "  ... and $((DEVICE_COUNT - 20)) more"
        fi
    fi

    echo "=============================================="
}

# Main execution
main() {
    check_deps
    find_bundle
    run_webcrack
    run_refactor
    print_summary
}

main "$@"

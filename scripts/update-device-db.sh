#!/bin/bash
# Update MonsGeek Device Database
#
# Fetches device data from:
#   1. app.monsgeek.com (webapp bundle)
#   2. Akko Cloud driver (Electron app) - optional
#
# Outputs unified database to: data/devices.json
#
# Usage:
#   ./update-device-db.sh              # Webapp only
#   ./update-device-db.sh --electron   # Include Electron driver
#   ./update-device-db.sh --clean      # Clean cached downloads

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
DRIVER_EXTRACT="$PROJECT_ROOT/driver_extract"
DATA_DIR="$PROJECT_ROOT/data"
CACHE_DIR="$PROJECT_ROOT/.cache/device-db"

WEBAPP_URL="https://app.monsgeek.com"
INCLUDE_ELECTRON=false
CLEAN_CACHE=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --electron)
            INCLUDE_ELECTRON=true
            shift
            ;;
        --clean)
            CLEAN_CACHE=true
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [options]"
            echo ""
            echo "Options:"
            echo "  --electron    Also extract from Electron driver (slower)"
            echo "  --clean       Clean cached downloads before fetching"
            echo "  --help        Show this help"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

echo "=============================================="
echo "MonsGeek Device Database Update"
echo "=============================================="
echo "Project root: $PROJECT_ROOT"
echo "Include Electron: $INCLUDE_ELECTRON"
echo ""

# Clean cache if requested
if [ "$CLEAN_CACHE" = true ]; then
    echo "Cleaning cache..."
    rm -rf "$CACHE_DIR"
fi

mkdir -p "$CACHE_DIR"
mkdir -p "$DATA_DIR"

# =============================================================================
# Step 1: Fetch webapp bundle
# =============================================================================

echo "=== Step 1: Fetching webapp bundle ==="

WEBAPP_DIR="$PROJECT_ROOT/app.monsgeek.com"
WEBAPP_BUNDLE=""

# Check if we have a recent local copy
if [ -d "$WEBAPP_DIR" ]; then
    WEBAPP_BUNDLE=$(find "$WEBAPP_DIR" -maxdepth 1 -name "index.*.js" -type f 2>/dev/null | head -1)
fi

if [ -z "$WEBAPP_BUNDLE" ] || [ ! -f "$WEBAPP_BUNDLE" ]; then
    echo "Fetching from $WEBAPP_URL..."

    # Get the HTML to find bundle name
    WEBAPP_HTML=$(curl -sL "$WEBAPP_URL")
    BUNDLE_NAME=$(echo "$WEBAPP_HTML" | grep -oP 'src="/\K[^"]+\.js' | head -1)

    if [ -z "$BUNDLE_NAME" ]; then
        # Try alternate pattern
        BUNDLE_NAME=$(echo "$WEBAPP_HTML" | grep -oP 'index\.[a-f0-9]+\.js' | head -1)
    fi

    if [ -z "$BUNDLE_NAME" ]; then
        echo "Error: Could not find bundle name in webapp HTML"
        echo "Falling back to cached/local version..."
    else
        mkdir -p "$WEBAPP_DIR"
        WEBAPP_BUNDLE="$WEBAPP_DIR/$BUNDLE_NAME"

        if [ ! -f "$WEBAPP_BUNDLE" ]; then
            echo "Downloading: $WEBAPP_URL/$BUNDLE_NAME"
            curl -sL "$WEBAPP_URL/$BUNDLE_NAME" -o "$WEBAPP_BUNDLE"
            echo "Downloaded: $WEBAPP_BUNDLE ($(du -h "$WEBAPP_BUNDLE" | cut -f1))"
        else
            echo "Using cached: $WEBAPP_BUNDLE"
        fi
    fi
else
    echo "Using local: $WEBAPP_BUNDLE"
fi

# Extract from webapp
WEBAPP_JSON="$CACHE_DIR/webapp_devices.json"

if [ -f "$WEBAPP_BUNDLE" ]; then
    echo "Extracting devices from webapp..."
    node "$DRIVER_EXTRACT/extract-devices.js" "$WEBAPP_BUNDLE" \
        --source "webapp" \
        --output "$WEBAPP_JSON"
    echo ""
else
    echo "Warning: No webapp bundle found, skipping webapp extraction"
fi

# =============================================================================
# Step 2: Extract from Electron driver (optional)
# =============================================================================

ELECTRON_JSON=""

if [ "$INCLUDE_ELECTRON" = true ]; then
    echo "=== Step 2: Extracting from Electron driver ==="

    # Check for existing extracted/deobfuscated bundle
    ELECTRON_BUNDLE=""

    # Look in refactored directories
    for dir in "$DRIVER_EXTRACT/unbundled" "$DRIVER_EXTRACT/refactored-v3/src"; do
        if [ -d "$dir" ]; then
            ELECTRON_BUNDLE=$(find "$dir" -name "deobfuscated.js" -o -name "main.jsx" 2>/dev/null | head -1)
            [ -n "$ELECTRON_BUNDLE" ] && break
        fi
    done

    # Try extracted bundle directly
    if [ -z "$ELECTRON_BUNDLE" ]; then
        ELECTRON_BUNDLE=$(find "$DRIVER_EXTRACT/extracted" -name "index.*.js" -type f 2>/dev/null | head -1)
    fi

    if [ -n "$ELECTRON_BUNDLE" ] && [ -f "$ELECTRON_BUNDLE" ]; then
        echo "Using existing bundle: $ELECTRON_BUNDLE"
        ELECTRON_JSON="$CACHE_DIR/electron_devices.json"

        node "$DRIVER_EXTRACT/extract-devices.js" "$ELECTRON_BUNDLE" \
            --source "electron" \
            --output "$ELECTRON_JSON"
        echo ""
    else
        echo "No Electron bundle found."
        echo "Run 'driver_extract/download-and-extract.sh' first to download and extract the driver."
        echo "Continuing with webapp data only..."
        echo ""
    fi
fi

# =============================================================================
# Step 3: Merge and output final database
# =============================================================================

echo "=== Step 3: Generating final database ==="

OUTPUT_FILE="$DATA_DIR/devices.json"

LAYOUTS_FILE="$DATA_DIR/key_layouts.json"
KEYCODES_FILE="$DATA_DIR/key_codes.json"

if [ -f "$WEBAPP_JSON" ] && [ -f "$ELECTRON_JSON" ]; then
    echo "Merging webapp and electron data..."
    node "$DRIVER_EXTRACT/extract-devices.js" --merge \
        "$WEBAPP_JSON" "$ELECTRON_JSON" \
        --output "$OUTPUT_FILE"
elif [ -f "$WEBAPP_JSON" ]; then
    echo "Using webapp data only..."
    cp "$WEBAPP_JSON" "$OUTPUT_FILE"
    # Also copy layouts and keycodes if available
    WEBAPP_LAYOUTS="${WEBAPP_JSON%.json}_layouts.json"
    WEBAPP_KEYCODES="${WEBAPP_JSON%.json}_keycodes.json"
    [ -f "$WEBAPP_LAYOUTS" ] && cp "$WEBAPP_LAYOUTS" "$LAYOUTS_FILE"
    [ -f "$WEBAPP_KEYCODES" ] && cp "$WEBAPP_KEYCODES" "$KEYCODES_FILE"
elif [ -f "$ELECTRON_JSON" ]; then
    echo "Using electron data only..."
    cp "$ELECTRON_JSON" "$OUTPUT_FILE"
    ELECTRON_LAYOUTS="${ELECTRON_JSON%.json}_layouts.json"
    ELECTRON_KEYCODES="${ELECTRON_JSON%.json}_keycodes.json"
    [ -f "$ELECTRON_LAYOUTS" ] && cp "$ELECTRON_LAYOUTS" "$LAYOUTS_FILE"
    [ -f "$ELECTRON_KEYCODES" ] && cp "$ELECTRON_KEYCODES" "$KEYCODES_FILE"
else
    echo "Error: No device data extracted!"
    exit 1
fi

# Copy extra files to data dir if merge created them
MERGED_LAYOUTS="${OUTPUT_FILE%.json}_layouts.json"
MERGED_KEYCODES="${OUTPUT_FILE%.json}_keycodes.json"
[ -f "$MERGED_LAYOUTS" ] && mv "$MERGED_LAYOUTS" "$LAYOUTS_FILE"
[ -f "$MERGED_KEYCODES" ] && mv "$MERGED_KEYCODES" "$KEYCODES_FILE"

echo ""

# =============================================================================
# Step 4: Extract LED matrices from utility classes
# =============================================================================

echo "=== Step 4: Extracting LED matrices ==="

MATRICES_FILE="$DATA_DIR/led_matrices.json"
MATRICES_SCRIPT="$DRIVER_EXTRACT/extract-matrices.js"

if [ -f "$MATRICES_SCRIPT" ]; then
    if [ -d "$DRIVER_EXTRACT/refactored-v3/src/utils" ]; then
        echo "Extracting matrices from device utility classes..."
        node "$MATRICES_SCRIPT"
        echo ""
    else
        echo "Warning: refactored-v3/src/utils not found, skipping matrix extraction"
        echo "Run driver_extract refactoring first to get utility classes."
        echo ""
    fi
else
    echo "Warning: extract-matrices.js not found, skipping matrix extraction"
    echo ""
fi

# =============================================================================
# Step 5: Merge device matrices (device ID -> matrix lookup)
# =============================================================================

echo "=== Step 5: Merging device matrices ==="

DEVICE_MATRICES_FILE="$DATA_DIR/device_matrices.json"
MERGE_SCRIPT="$DRIVER_EXTRACT/merge-matrices.js"

if [ -f "$MERGE_SCRIPT" ] && [ -f "$MATRICES_FILE" ]; then
    echo "Creating device ID to matrix lookup..."
    node "$MERGE_SCRIPT"
    echo ""
else
    echo "Warning: Cannot merge matrices (missing merge-matrices.js or led_matrices.json)"
    echo ""
fi

# =============================================================================
# Summary
# =============================================================================

echo "=============================================="
echo "UPDATE COMPLETE"
echo "=============================================="
echo ""
echo "Output files:"
echo "  Devices: $OUTPUT_FILE"
[ -f "$LAYOUTS_FILE" ] && echo "  Layouts: $LAYOUTS_FILE"
[ -f "$KEYCODES_FILE" ] && echo "  KeyCodes: $KEYCODES_FILE"
[ -f "$MATRICES_FILE" ] && echo "  Matrices: $MATRICES_FILE"
[ -f "$DEVICE_MATRICES_FILE" ] && echo "  Device matrices: $DEVICE_MATRICES_FILE"

if [ -f "$OUTPUT_FILE" ]; then
    DEVICE_COUNT=$(grep -c '"id":' "$OUTPUT_FILE" || echo "?")
    FILE_SIZE=$(du -h "$OUTPUT_FILE" | cut -f1)
    echo ""
    echo "Devices: $DEVICE_COUNT ($FILE_SIZE)"
fi

if [ -f "$LAYOUTS_FILE" ]; then
    LAYOUT_COUNT=$(node -e "console.log(require('$LAYOUTS_FILE').count)" 2>/dev/null || echo "?")
    LAYOUT_SIZE=$(du -h "$LAYOUTS_FILE" | cut -f1)
    echo "Key layouts: $LAYOUT_COUNT ($LAYOUT_SIZE)"
fi

if [ -f "$KEYCODES_FILE" ]; then
    KEYCODE_COUNT=$(node -e "const d=require('$KEYCODES_FILE'); console.log(d.arrayCount + ' arrays, ' + d.mappingCount + ' mappings')" 2>/dev/null || echo "?")
    KEYCODE_SIZE=$(du -h "$KEYCODES_FILE" | cut -f1)
    echo "KeyCode arrays: $KEYCODE_COUNT ($KEYCODE_SIZE)"
fi

if [ -f "$MATRICES_FILE" ]; then
    MATRIX_COUNT=$(node -e "console.log(require('$MATRICES_FILE').stats.totalDevices)" 2>/dev/null || echo "?")
    MATRIX_SIZE=$(du -h "$MATRICES_FILE" | cut -f1)
    echo "LED matrices: $MATRIX_COUNT ($MATRIX_SIZE)"
fi

if [ -f "$DEVICE_MATRICES_FILE" ]; then
    DEVMATRIX_COUNT=$(node -e "console.log(require('$DEVICE_MATRICES_FILE').stats.matched)" 2>/dev/null || echo "?")
    DEVMATRIX_SIZE=$(du -h "$DEVICE_MATRICES_FILE" | cut -f1)
    echo "Device matrices: $DEVMATRIX_COUNT ($DEVMATRIX_SIZE)"
fi

# Show sample
if [ -f "$OUTPUT_FILE" ]; then
    echo ""
    echo "Sample device:"
    node -e "
        const db = require('$OUTPUT_FILE');
        const dev = db.devices?.find(d => d.displayName?.includes('M1') && d.displayName?.includes('TMR'));
        if (dev) {
            console.log(JSON.stringify(dev, null, 2));
        } else {
            const fallback = db.devices?.find(d => d.displayName?.includes('M1'));
            if (fallback) console.log(JSON.stringify(fallback, null, 2));
        }
    " 2>/dev/null || true
fi

echo ""
echo "=============================================="

#!/bin/bash
# Update the MonsGeek/Akko device database.
#
# Mines device definitions, key matrices and key layouts from any number of sources and
# merges them into data/devices.json + data/device_matrices.json:
#
#   webapp   app.monsgeek.com bundle (always, unless --no-webapp)
#   vendor   an Electron driver unpacked by driver_extract/download-and-extract.sh into
#            driver_extract/vendors/<tag>/ — repeatable
#
# Rebranded vendor drivers (WOMIER, Epomaker, ...) ship the same app with their own models
# added, so a locally downloaded vendor installer is often the only source for a recently
# released keyboard. Earlier sources win on conflict; --vendor order is the priority order.
#
# Usage:
#   ./update-device-db.sh                              # webapp only
#   ./update-device-db.sh --electron                   # + the "akko" vendor workspace
#   ./update-device-db.sh --vendor akko --vendor womier
#   ./update-device-db.sh --no-webapp --vendor womier  # local driver only
#   ./update-device-db.sh --clean                      # drop cached intermediates first

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
DRIVER_EXTRACT="$PROJECT_ROOT/driver_extract"
DATA_DIR="$PROJECT_ROOT/data"
CACHE_DIR="$PROJECT_ROOT/.cache/device-db"

WEBAPP_URL="https://app.monsgeek.com"
INCLUDE_WEBAPP=true
CLEAN_CACHE=false
VENDORS=()

while [[ $# -gt 0 ]]; do
    case $1 in
        --electron)
            VENDORS+=("akko")
            shift
            ;;
        --vendor)
            VENDORS+=("$2")
            shift 2
            ;;
        --no-webapp)
            INCLUDE_WEBAPP=false
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
            echo "  --vendor <tag>  Include driver_extract/vendors/<tag> (repeatable)"
            echo "  --electron      Shorthand for --vendor akko"
            echo "  --no-webapp     Skip the app.monsgeek.com bundle"
            echo "  --clean         Clean cached intermediates before extracting"
            echo "  --help          Show this help"
            echo ""
            echo "Unpack a vendor driver first with:"
            echo "  driver_extract/download-and-extract.sh --input <installer> --vendor <tag>"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

if [ "$INCLUDE_WEBAPP" = false ] && [ ${#VENDORS[@]} -eq 0 ]; then
    echo "Error: nothing to extract — --no-webapp with no --vendor"
    exit 1
fi

echo "=============================================="
echo "MonsGeek Device Database Update"
echo "=============================================="
echo "Project root: $PROJECT_ROOT"
echo "Webapp: $INCLUDE_WEBAPP"
echo "Vendors: ${VENDORS[*]:-none}"
echo ""

if [ "$CLEAN_CACHE" = true ]; then
    echo "Cleaning cache..."
    rm -rf "$CACHE_DIR"
fi

mkdir -p "$CACHE_DIR" "$DATA_DIR"

# Device JSONs in merge priority order, and the per-vendor LED matrix files / SVG dirs
# that feed merge-matrices.js.
DEVICE_JSONS=()
MATRIX_ARGS=()
SVG_ARGS=()

# =============================================================================
# Step 1: webapp bundle
# =============================================================================

extract_webapp() {
    echo "=== Step 1: Fetching webapp bundle ==="

    local webapp_dir="$PROJECT_ROOT/app.monsgeek.com"
    local bundle=""

    if [ -d "$webapp_dir" ]; then
        bundle=$(find "$webapp_dir" -maxdepth 1 -name "index.*.js" -type f 2>/dev/null | head -1)
    fi

    if [ -z "$bundle" ] || [ ! -f "$bundle" ]; then
        echo "Fetching from $WEBAPP_URL..."
        local html name
        html=$(curl -sL "$WEBAPP_URL")
        name=$(echo "$html" | grep -oP 'src="/\K[^"]+\.js' | head -1)
        [ -z "$name" ] && name=$(echo "$html" | grep -oP 'index\.[a-f0-9]+\.js' | head -1)

        if [ -z "$name" ]; then
            echo "Warning: could not find the bundle name in the webapp HTML — skipping webapp"
            return
        fi

        mkdir -p "$webapp_dir"
        bundle="$webapp_dir/$name"
        if [ ! -f "$bundle" ]; then
            echo "Downloading: $WEBAPP_URL/$name"
            curl -sL "$WEBAPP_URL/$name" -o "$bundle"
        fi
    fi
    echo "Bundle: $bundle ($(du -h "$bundle" | cut -f1))"

    local out="$CACHE_DIR/webapp_devices.json"
    node "$DRIVER_EXTRACT/extract-devices.js" "$bundle" --source webapp --output "$out"
    DEVICE_JSONS+=("$out")
    echo ""
}

# =============================================================================
# Step 2: vendor Electron drivers
# =============================================================================

extract_vendor() {
    local tag="$1"
    local workspace="$DRIVER_EXTRACT/vendors/$tag"
    local manifest="$workspace/manifest.json"

    echo "=== Vendor '$tag' ==="

    if [ ! -f "$manifest" ]; then
        echo "Warning: no workspace at $workspace — skipping."
        echo "  Unpack it first: driver_extract/download-and-extract.sh --input <installer> --vendor $tag"
        echo ""
        return
    fi

    local format bundle chunks_dir refactored svg_dir
    format=$(node -p "require('$manifest').format")
    bundle=$(node -p "require('$manifest').bundle || ''")
    chunks_dir=$(node -p "require('$manifest').chunksDir || ''")
    refactored=$(node -p "require('$manifest').refactoredDir || ''")
    svg_dir=$(node -p "require('$manifest').svgDir || ''")

    echo "Format: $format"

    # Devices, key layouts and keycodes come from the entry bundle in both formats.
    if [ -n "$bundle" ]; then
        local out="$CACHE_DIR/${tag}_devices.json"
        node "$DRIVER_EXTRACT/extract-devices.js" "$bundle" --source "$tag" --output "$out"
        DEVICE_JSONS+=("$out")
    else
        echo "Warning: manifest has no bundle — no devices from '$tag'"
    fi

    # Matrices come from the per-device driver classes.
    local matrices="$CACHE_DIR/${tag}_led_matrices.json"
    if [ "$format" = "chunks" ] && [ -n "$chunks_dir" ]; then
        node "$DRIVER_EXTRACT/extract-matrices.js" --chunks "$chunks_dir" -o "$matrices"
        MATRIX_ARGS+=(--matrices "$matrices")
    elif [ "$format" = "monolithic" ] && [ -n "$refactored" ]; then
        node "$DRIVER_EXTRACT/extract-matrices.js" --refactored "$refactored" -o "$matrices"
        MATRIX_ARGS+=(--matrices "$matrices")
    else
        echo "Warning: no matrix source for '$tag'"
    fi

    [ -n "$svg_dir" ] && SVG_ARGS+=(--svg-dir "$svg_dir")
    echo ""
}

[ "$INCLUDE_WEBAPP" = true ] && extract_webapp
for tag in "${VENDORS[@]}"; do
    extract_vendor "$tag"
done

# =============================================================================
# Step 3: merge device definitions
# =============================================================================

echo "=== Merging device definitions ==="

DEVICES_FILE="$DATA_DIR/devices.json"
LAYOUTS_FILE="$DATA_DIR/key_layouts.json"
KEYCODES_FILE="$DATA_DIR/key_codes.json"

if [ ${#DEVICE_JSONS[@]} -eq 0 ]; then
    echo "Error: no device data extracted!"
    exit 1
elif [ ${#DEVICE_JSONS[@]} -eq 1 ]; then
    cp "${DEVICE_JSONS[0]}" "$DEVICES_FILE"
    src="${DEVICE_JSONS[0]%.json}"
    [ -f "${src}_layouts.json" ] && cp "${src}_layouts.json" "$LAYOUTS_FILE"
    [ -f "${src}_keycodes.json" ] && cp "${src}_keycodes.json" "$KEYCODES_FILE"
else
    node "$DRIVER_EXTRACT/extract-devices.js" --merge "${DEVICE_JSONS[@]}" --output "$DEVICES_FILE"
    merged_layouts="${DEVICES_FILE%.json}_layouts.json"
    [ -f "$merged_layouts" ] && mv "$merged_layouts" "$LAYOUTS_FILE"
    # The merge does not combine keycode arrays; keep the richest single source.
    best=$(ls -S "$CACHE_DIR"/*_keycodes.json 2>/dev/null | head -1)
    [ -n "$best" ] && cp "$best" "$KEYCODES_FILE"
fi
echo ""

# =============================================================================
# Step 4: merge matrices into the device ID lookup
# =============================================================================

echo "=== Merging key matrices ==="

MATRICES_FILE="$DATA_DIR/device_matrices.json"

if [ ${#MATRIX_ARGS[@]} -eq 0 ]; then
    echo "No vendor matrix data — leaving $MATRICES_FILE untouched."
    echo "  (Matrices only come from an unpacked vendor driver, not the webapp bundle.)"
else
    node "$DRIVER_EXTRACT/merge-matrices.js" \
        --devices "$DEVICES_FILE" \
        "${MATRIX_ARGS[@]}" \
        "${SVG_ARGS[@]}" \
        -o "$MATRICES_FILE"
fi
echo ""

# =============================================================================
# Summary
# =============================================================================

echo "=============================================="
echo "UPDATE COMPLETE"
echo "=============================================="
node -e '
    const fs = require("fs");
    const show = (label, file, fn) => {
        if (!fs.existsSync(file)) return;
        const size = (fs.statSync(file).size / 1024 / 1024).toFixed(1) + " MB";
        console.log(`  ${label}: ${fn(JSON.parse(fs.readFileSync(file)))} (${size})`);
    };
    const [devices, layouts, keycodes, matrices] = process.argv.slice(1);
    show("Devices", devices, d => `${d.devices.length} devices`);
    show("Key layouts", layouts, d => `${d.count} layouts`);
    show("KeyCodes", keycodes, d => `${d.arrayCount} arrays, ${d.mappingCount} mappings`);
    show("Device matrices", matrices, d => `${d.stats.matched}/${d.stats.totalKeyboards} keyboards`);
' "$DEVICES_FILE" "$LAYOUTS_FILE" "$KEYCODES_FILE" "$MATRICES_FILE"
echo "=============================================="

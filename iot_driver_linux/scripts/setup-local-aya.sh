#!/bin/bash
# Setup local aya checkout for development
# This creates a .cargo/config.toml at the repo root that patches aya dependencies
# to use local paths instead of the GitHub fork.
#
# Usage: ./scripts/setup-local-aya.sh [path-to-aya]
# Default path: ../../aya/aya-main (relative to repo root)
#
# To remove: rm -rf .cargo/

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Default aya path (relative to repo root)
AYA_PATH="${1:-$REPO_ROOT/../../aya/aya-main}"

# Convert to absolute path
AYA_PATH="$(cd "$AYA_PATH" 2>/dev/null && pwd)" || {
    echo "Error: aya checkout not found at: $1"
    echo "Usage: $0 [path-to-aya-checkout]"
    echo "Expected structure: path/aya/aya-main/ with aya/, aya-obj/, ebpf/aya-ebpf/"
    exit 1
}

echo "Using aya checkout at: $AYA_PATH"

# Verify aya structure
for subdir in "aya" "aya-obj" "ebpf/aya-ebpf"; do
    if [ ! -d "$AYA_PATH/$subdir" ]; then
        echo "Error: Missing $AYA_PATH/$subdir"
        exit 1
    fi
done

echo "Setting up local aya patches..."

# Calculate relative path from repo root to aya
AYA_REL_PATH=$(realpath --relative-to="$REPO_ROOT" "$AYA_PATH")

# Create .cargo/config.toml at repo root with all patches
mkdir -p "$REPO_ROOT/.cargo"
cat > "$REPO_ROOT/.cargo/config.toml" << EOF
# Local aya patches for development (created by scripts/setup-local-aya.sh)
# This file is gitignored - remove with: rm -rf .cargo/

[patch."https://github.com/heeen/aya.git"]
# For akko-ebpf
aya-ebpf = { path = "$AYA_REL_PATH/ebpf/aya-ebpf" }
# For akko-loader-rs
aya = { path = "$AYA_REL_PATH/aya" }
aya-obj = { path = "$AYA_REL_PATH/aya-obj" }
EOF

echo "Created .cargo/config.toml with local aya patches"
echo ""
echo "Done! Local aya patches configured."
echo "To remove patches, run: rm -rf .cargo/"

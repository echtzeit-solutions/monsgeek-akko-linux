#!/usr/bin/env bash
# Run ExportSymbols.java against a program in the Ghidra Server repository.
#
# The server repository is the source of truth for names; this reads the latest
# checked-in version read-only, so the export can never modify it. Rename in
# Ghidra, check in, then run this (usually via `make generate` in patch/).
#
# Usage: ghidra_export_symbols.sh <repo> <program> <script-dir> <output.json>
#
# Credentials come from the same env file the ghidra-headless@ services use
# (GHIDRA_SERVER_HOST / _PORT / _USER / _PASSWORD). Override its location with
# GHIDRA_SERVER_ENV.
set -euo pipefail

if [ "$#" -ne 4 ]; then
    echo "usage: $0 <repo> <program> <script-dir> <output.json>" >&2
    exit 2
fi
repo=$1 program=$2 script_dir=$3 out=$4

env_file=${GHIDRA_SERVER_ENV:-$HOME/.config/ghidra-mcp/server.env}
[ -r "$env_file" ] || { echo "cannot read Ghidra server credentials: $env_file" >&2; exit 1; }
set -a
# shellcheck disable=SC1090
. "$env_file"
set +a
: "${GHIDRA_SERVER_HOST:?missing in $env_file}"
: "${GHIDRA_SERVER_USER:?missing in $env_file}"
: "${GHIDRA_SERVER_PASSWORD:?missing in $env_file}"
port=${GHIDRA_SERVER_PORT:-13100}

analyze_headless=${GHIDRA:-${GHIDRA_HOME:-/opt/ghidra}/support/analyzeHeadless}

# analyzeHeadless has no non-interactive password option; with -p it reads the
# password from stdin when there is no terminal.
printf '%s\n' "$GHIDRA_SERVER_PASSWORD" | "$analyze_headless" \
    "ghidra://$GHIDRA_SERVER_HOST:$port/$repo" \
    -connect "$GHIDRA_SERVER_USER" -p -readOnly \
    -process "$program" -noanalysis \
    -scriptPath "$script_dir" \
    -postScript ExportSymbols.java "$out"

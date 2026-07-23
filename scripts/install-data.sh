#!/usr/bin/env bash
# Install the device database (devices.json + device_matrices.json) to DATA_DIR.
#
# When run interactively, first offers (default Yes) to refresh the database from the
# vendor driver so freshly-released models are picked up. The refresh is best-effort:
# on a declined prompt, missing tools, no network, or invalid output it silently falls
# back to the committed assets — it never blocks the install.
#
# Env knobs:
#   DATA_DIR    install destination (default /usr/local/share/akko)
#   INSTALL     install(1) program (default "install")
#   ASSUME_YES  set to 1 to refresh without prompting (for scripted installs)
#   SKIP_REFRESH set to 1 to never refresh (install committed assets as-is)
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA_DIR="${DATA_DIR:-/usr/local/share/akko}"
INSTALL="${INSTALL:-install}"

DEVICES="$REPO_ROOT/data/devices.json"
MATRICES="$REPO_ROOT/data/device_matrices.json"

# Run a command as the invoking user when started via sudo, so the refresh's downloads
# and node/babel scratch files don't end up root-owned inside the repo.
as_user() {
  if [ "$(id -u)" -eq 0 ] && [ -n "${SUDO_USER:-}" ]; then
    sudo -u "$SUDO_USER" "$@"
  else
    "$@"
  fi
}

# The Electron path (the only complete matrix source) needs these tools.
refresh_possible() {
  command -v node >/dev/null 2>&1 && command -v npm >/dev/null 2>&1 &&
    command -v curl >/dev/null 2>&1 && command -v 7z >/dev/null 2>&1
}

# Reject output that is broken or thinner than what we already ship. The committed
# database is merged from several vendor drivers; a refresh that can only reach one of
# them (say the webapp, because no vendor workspace is unpacked) would otherwise quietly
# install a smaller device list than the one in the repo.
validate_db() {
  local backup="$1"
  node -e '
    const fs = require("fs");
    const count = (file, pick) => {
      const j = JSON.parse(fs.readFileSync(file));
      const v = pick(j);
      return Array.isArray(v) ? v.length : Object.keys(v).length;
    };
    const [devices, matrices, backupDir] = process.argv.slice(1);
    const nd = count(devices, j => j.devices || j);
    const nm = count(matrices, j => j.devices || {});
    if (!(nd >= 300)) throw new Error("device count too low: " + nd);
    if (!(nm >= 300)) throw new Error("matrix count too low: " + nm);

    for (const [label, name, now] of [["device", "devices.json", nd], ["matrix", "device_matrices.json", nm]]) {
      const prev = backupDir + "/" + name;
      if (!fs.existsSync(prev)) continue;
      const before = count(prev, j => j.devices || j);
      if (now < before) throw new Error(`${label} count regressed: ${before} -> ${now}`);
    }
  ' "$DEVICES" "$MATRICES" "$backup"
}

do_refresh() {
  echo "Refreshing device database from the vendor driver..."
  echo "  (downloads the Akko Cloud installer ~70MB + runs the extractor; may take a few minutes)"
  local backup
  backup="$(mktemp -d)"
  # The update also rewrites key_layouts/key_codes; back them up too so a rejected
  # refresh leaves the working tree exactly as it found it.
  local regenerated=(devices.json device_matrices.json key_layouts.json key_codes.json)
  for f in "${regenerated[@]}"; do cp "$REPO_ROOT/data/$f" "$backup/" 2>/dev/null || true; done

  if as_user "$REPO_ROOT/scripts/update-device-db.sh" --electron && validate_db "$backup"; then
    echo "Device database refreshed and validated."
  else
    echo "WARNING: refresh failed or produced invalid data — keeping the committed database." >&2
    for f in "${regenerated[@]}"; do cp "$backup/$f" "$REPO_ROOT/data/$f" 2>/dev/null || true; done
  fi
  rm -rf "$backup"
}

# Decide whether to refresh.
refresh=no
if [ "${SKIP_REFRESH:-}" = "1" ]; then
  refresh=no
elif [ "${ASSUME_YES:-}" = "1" ]; then
  refresh_possible && refresh=yes ||
    echo "Note: ASSUME_YES set but refresh tools (node/npm/curl/7z) missing — installing committed database."
elif [ -t 0 ]; then
  if refresh_possible; then
    read -r -p "Refresh device database from the vendor driver before installing? [Y/n] " ans
    case "${ans:-Y}" in [Nn]*) refresh=no ;; *) refresh=yes ;; esac
  else
    echo "Note: refresh tools (node/npm/curl/7z) not all present — installing committed database."
  fi
fi

[ "$refresh" = yes ] && do_refresh

# Committed assets are the floor: they must exist either way.
if [ ! -f "$DEVICES" ] || [ ! -f "$MATRICES" ]; then
  echo "Error: device data files missing under $REPO_ROOT/data." >&2
  echo "       Run 'make update-device-db-full' to generate them." >&2
  exit 1
fi

echo "Installing device data to $DATA_DIR ..."
$INSTALL -d "$DATA_DIR"
$INSTALL -m 644 "$DEVICES" "$DATA_DIR/devices.json"
$INSTALL -m 644 "$MATRICES" "$DATA_DIR/device_matrices.json"
echo "Device data installed to $DATA_DIR"

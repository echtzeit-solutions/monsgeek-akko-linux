#!/bin/bash
# MonsGeek M1 V5 HE Driver - VM Setup Script
# Creates a QEMU/KVM VM for testing the driver in Ubuntu 25.10
#
# Prerequisites:
#   sudo apt install qemu-kvm libvirt-daemon-system virt-manager
#   sudo usermod -aG libvirt,kvm $USER

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$(dirname "$SCRIPT_DIR")")"

# Configuration
VM_NAME="${VM_NAME:-monsgeek-test}"
VM_RAM="${VM_RAM:-8192}"  # MB
VM_CPUS="${VM_CPUS:-4}"
VM_DISK="${VM_DISK:-40}"  # GB
ISO_PATH="${ISO_PATH:-}"
DISK_PATH="${DISK_PATH:-$HOME/.local/share/libvirt/images/${VM_NAME}.qcow2}"

# MonsGeek USB device IDs
USB_WIRED_VID="3151"
USB_WIRED_PID="5030"
USB_DONGLE_VID="3151"
USB_DONGLE_PID="5038"

log_info() { echo "[INFO] $*"; }
log_warn() { echo "[WARN] $*"; }
log_error() { echo "[ERROR] $*" >&2; }

check_deps() {
    local missing=()

    for cmd in virsh virt-install qemu-img; do
        if ! command -v "$cmd" &>/dev/null; then
            missing+=("$cmd")
        fi
    done

    if [[ ${#missing[@]} -gt 0 ]]; then
        log_error "Missing dependencies: ${missing[*]}"
        echo "Install with: sudo apt install qemu-kvm libvirt-daemon-system virtinst"
        exit 1
    fi

    # Check libvirt is running
    if ! systemctl is-active --quiet libvirtd; then
        log_error "libvirtd is not running"
        echo "Start with: sudo systemctl start libvirtd"
        exit 1
    fi

    # Check user is in libvirt group
    if ! groups | grep -qE 'libvirt|kvm'; then
        log_warn "User not in libvirt/kvm groups. May need: sudo usermod -aG libvirt,kvm \$USER"
    fi
}

create_vm() {
    if [[ -z "$ISO_PATH" ]] || [[ ! -f "$ISO_PATH" ]]; then
        log_error "ISO_PATH not set or file not found"
        echo "Download Ubuntu 25.10 from: https://releases.ubuntu.com/"
        echo "Then run: ISO_PATH=/path/to/ubuntu.iso $0"
        exit 1
    fi

    # Create disk directory
    mkdir -p "$(dirname "$DISK_PATH")"

    # Create disk image
    if [[ ! -f "$DISK_PATH" ]]; then
        log_info "Creating disk image: $DISK_PATH (${VM_DISK}G)"
        qemu-img create -f qcow2 "$DISK_PATH" "${VM_DISK}G"
    else
        log_warn "Disk already exists: $DISK_PATH"
    fi

    # Check if VM already exists
    if virsh list --all --name | grep -q "^${VM_NAME}$"; then
        log_warn "VM '$VM_NAME' already exists"
        echo "Delete with: virsh destroy $VM_NAME; virsh undefine $VM_NAME --remove-all-storage"
        exit 1
    fi

    log_info "Creating VM: $VM_NAME"

    virt-install \
        --name "$VM_NAME" \
        --ram "$VM_RAM" \
        --vcpus "$VM_CPUS" \
        --cpu host-passthrough \
        --disk "path=$DISK_PATH,format=qcow2,bus=virtio" \
        --cdrom "$ISO_PATH" \
        --os-variant ubuntu24.10 \
        --network network=default,model=virtio \
        --graphics spice \
        --video qxl \
        --channel spicevmc,target_type=virtio,name=com.redhat.spice.0 \
        --boot uefi \
        --noautoconsole

    log_info "VM created. Complete installation via virt-manager."
    echo ""
    echo "Next steps:"
    echo "1. Open virt-manager and complete Ubuntu installation"
    echo "2. After install, run: $0 usb-passthrough"
    echo "3. Run: $0 guest-setup"
}

usb_passthrough() {
    log_info "Adding USB passthrough for MonsGeek devices..."

    # Check if VM exists
    if ! virsh list --all --name | grep -q "^${VM_NAME}$"; then
        log_error "VM '$VM_NAME' not found"
        exit 1
    fi

    # Stop VM if running
    if virsh list --name | grep -q "^${VM_NAME}$"; then
        log_info "Stopping VM for USB configuration..."
        virsh shutdown "$VM_NAME"
        sleep 5
        virsh destroy "$VM_NAME" 2>/dev/null || true
    fi

    # Add USB controller (XHCI for USB 3.0)
    local usb_xml=$(mktemp)
    cat > "$usb_xml" <<EOF
<controller type='usb' model='qemu-xhci'>
  <alias name='usb'/>
</controller>
EOF
    virsh attach-device "$VM_NAME" "$usb_xml" --config 2>/dev/null || log_warn "USB controller already configured"
    rm "$usb_xml"

    # Add wired keyboard
    local wired_xml=$(mktemp)
    cat > "$wired_xml" <<EOF
<hostdev mode='subsystem' type='usb' managed='yes'>
  <source>
    <vendor id='0x${USB_WIRED_VID}'/>
    <product id='0x${USB_WIRED_PID}'/>
  </source>
  <address type='usb' bus='0'/>
</hostdev>
EOF

    log_info "Adding wired keyboard passthrough (${USB_WIRED_VID}:${USB_WIRED_PID})"
    virsh attach-device "$VM_NAME" "$wired_xml" --config 2>/dev/null || log_warn "Wired device already added or not present"
    rm "$wired_xml"

    # Add dongle
    local dongle_xml=$(mktemp)
    cat > "$dongle_xml" <<EOF
<hostdev mode='subsystem' type='usb' managed='yes'>
  <source>
    <vendor id='0x${USB_DONGLE_VID}'/>
    <product id='0x${USB_DONGLE_PID}'/>
  </source>
  <address type='usb' bus='0'/>
</hostdev>
EOF

    log_info "Adding dongle passthrough (${USB_DONGLE_VID}:${USB_DONGLE_PID})"
    virsh attach-device "$VM_NAME" "$dongle_xml" --config 2>/dev/null || log_warn "Dongle already added or not present"
    rm "$dongle_xml"

    log_info "USB passthrough configured"
    echo ""
    echo "Start VM with: virsh start $VM_NAME"
    echo "Connect with: virt-viewer $VM_NAME"
}

guest_setup_script() {
    # Generate a script to run inside the guest VM
    cat <<'GUEST_SCRIPT'
#!/bin/bash
# Guest VM setup script for MonsGeek driver testing
# Run this inside the Ubuntu 25.10 VM

set -euo pipefail

log_info() { echo "[INFO] $*"; }
log_error() { echo "[ERROR] $*" >&2; }

# Check we're in the VM
if [[ ! -f /etc/os-release ]] || ! grep -q "Ubuntu" /etc/os-release; then
    log_error "This script should be run inside Ubuntu VM"
    exit 1
fi

log_info "Installing build dependencies..."
sudo apt update
sudo apt install -y \
    build-essential pkg-config \
    libudev-dev libhidapi-dev \
    protobuf-compiler \
    libasound2-dev libdbus-1-dev \
    libclang-dev libelf-dev zlib1g-dev \
    git curl

# Install Rust
if ! command -v rustc &>/dev/null; then
    log_info "Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

# Install nightly for BPF
log_info "Installing Rust nightly..."
rustup install nightly
rustup component add rust-src --toolchain nightly

# Install bpf-linker
log_info "Installing bpf-linker..."
cargo install bpf-linker || log_info "bpf-linker may require manual setup"

# Check kernel version
kernel=$(uname -r | cut -d. -f1-2)
log_info "Kernel version: $kernel"

if [[ $(echo "$kernel >= 6.12" | bc -l) -eq 1 ]]; then
    log_info "Kernel supports HID-BPF struct_ops"
else
    log_error "Kernel $kernel may not support HID-BPF (need 6.12+)"
fi

# Check for MonsGeek devices
log_info "Checking USB devices..."
if lsusb | grep -q "3151:"; then
    log_info "MonsGeek device detected:"
    lsusb | grep "3151:"
else
    log_error "No MonsGeek device detected - check USB passthrough"
fi

log_info ""
log_info "Setup complete!"
log_info "Clone the repository and run tests:"
log_info "  git clone <repo> monsgeek"
log_info "  cd monsgeek"
log_info "  make driver && ./tests/run_tests.sh"
GUEST_SCRIPT
}

generate_guest_script() {
    local output="${1:-$SCRIPT_DIR/guest_setup.sh}"
    guest_setup_script > "$output"
    chmod +x "$output"
    log_info "Generated guest setup script: $output"
    echo "Copy to VM and run: scp $output user@vm:~ && ssh user@vm ./guest_setup.sh"
}

snapshot() {
    local name="${1:-clean-install}"

    if ! virsh list --all --name | grep -q "^${VM_NAME}$"; then
        log_error "VM '$VM_NAME' not found"
        exit 1
    fi

    log_info "Creating snapshot: $name"
    virsh snapshot-create-as "$VM_NAME" --name "$name" --description "Test snapshot"
    log_info "Snapshot created. Restore with: virsh snapshot-revert $VM_NAME $name"
}

list_snapshots() {
    virsh snapshot-list "$VM_NAME"
}

usage() {
    echo "MonsGeek Driver VM Setup"
    echo ""
    echo "Usage: $0 <command> [options]"
    echo ""
    echo "Commands:"
    echo "  create            Create new VM (requires ISO_PATH)"
    echo "  usb-passthrough   Configure USB passthrough for MonsGeek devices"
    echo "  guest-setup       Generate guest VM setup script"
    echo "  snapshot [name]   Create VM snapshot"
    echo "  snapshots         List snapshots"
    echo ""
    echo "Environment:"
    echo "  VM_NAME=$VM_NAME"
    echo "  VM_RAM=$VM_RAM MB"
    echo "  VM_CPUS=$VM_CPUS"
    echo "  VM_DISK=$VM_DISK GB"
    echo "  ISO_PATH=${ISO_PATH:-<not set>}"
    echo ""
    echo "Example:"
    echo "  ISO_PATH=/path/to/ubuntu-25.10.iso $0 create"
    echo "  $0 usb-passthrough"
    echo "  $0 guest-setup"
}

main() {
    local cmd="${1:-help}"

    case "$cmd" in
        create)
            check_deps
            create_vm
            ;;
        usb-passthrough|usb)
            check_deps
            usb_passthrough
            ;;
        guest-setup|guest)
            generate_guest_script "${2:-}"
            ;;
        snapshot|snap)
            check_deps
            snapshot "${2:-}"
            ;;
        snapshots|list)
            check_deps
            list_snapshots
            ;;
        help|--help|-h)
            usage
            ;;
        *)
            log_error "Unknown command: $cmd"
            usage
            exit 1
            ;;
    esac
}

main "$@"

#!/usr/bin/env python3
from __future__ import annotations

import argparse
import binascii
import ctypes
import os
import select
import signal
import sys
import time
from dataclasses import dataclass
from pathlib import Path


# uhid.h constants
UHID_DESTROY = 1
UHID_START = 2
UHID_STOP = 3
UHID_OPEN = 4
UHID_CLOSE = 5
UHID_OUTPUT = 6
UHID_GET_REPORT = 9
UHID_GET_REPORT_REPLY = 10
UHID_CREATE2 = 11
UHID_INPUT2 = 12
UHID_SET_REPORT = 13
UHID_SET_REPORT_REPLY = 14

UHID_FEATURE_REPORT = 0
UHID_OUTPUT_REPORT = 1
UHID_INPUT_REPORT = 2

BUS_USB = 0x03
UHID_DATA_MAX = 4096

# Known command names from protocol.rs and firmware analysis
COMMAND_NAMES: dict[int, str] = {
    0x01: "SET_RESET",
    0x03: "SET_REPORT",
    0x04: "SET_PROFILE",
    0x06: "SET_DEBOUNCE",
    0x07: "SET_LEDPARAM",
    0x08: "SET_SLEDPARAM",
    0x09: "SET_KBOPTION",
    0x0A: "SET_KEYMATRIX",
    0x0B: "SET_MACRO",
    0x0C: "SET_USERPIC",
    0x10: "SET_FN",
    0x11: "SET_SLEEPTIME",
    0x17: "SET_AUTOOS_EN",
    0x1D: "SET_KEY_MAGNETISM_MODE",
    0x65: "SET_MULTI_MAGNETISM",
    0x7F: "ENTER_BOOTLOADER",
    0x80: "GET_REV",
    0x83: "GET_REPORT",
    0x84: "GET_PROFILE",
    0x85: "GET_LEDONOFF",
    0x86: "GET_DEBOUNCE",
    0x87: "GET_LEDPARAM",
    0x88: "GET_SLEDPARAM",
    0x89: "GET_KBOPTION",
    0x8A: "GET_KEYMATRIX",
    0x8B: "GET_MACRO",
    0x8C: "GET_USERPIC",
    0x8F: "GET_USB_VERSION",
    0x90: "GET_FN",
    0x91: "GET_SLEEPTIME",
    0x97: "GET_AUTOOS_EN",
    0x9D: "GET_KEY_MAGNETISM_MODE",
    0xAD: "GET_OLED_VERSION",
    0xAE: "GET_MLED_VERSION",
    0xAA: "STATUS_SUCCESS",
    0xBA: "FW_TRANSFER",
    0xC5: "ISP_PREPARE",
    0xE5: "GET_MULTI_MAGNETISM",
    0xE6: "GET_FEATURE_LIST",
    0xF7: "BATTERY_REFRESH",
    0xFC: "DONGLE_FLUSH_NOP",
    0xFE: "GET_CALIBRATION",
}


def cmd_name(cmd: int | None) -> str:
    """Return human-readable name for a command byte."""
    if cmd is None:
        return "none"
    name = COMMAND_NAMES.get(cmd)
    if name:
        return f"{name}(0x{cmd:02x})"
    return f"0x{cmd:02x}"


# Interface 2: Vendor config descriptor - 64-byte feature reports (Usage Page 0xFFFF, Usage 0x02)
REPORT_DESCRIPTOR_IF2 = bytes.fromhex(
    "06ff" "ff" "0902" "a101" "0902" "1580" "257f" "9540" "7508" "b102" "c0"
)

# Bootloader descriptor: usage_page 0xFF01, usage 0x01, 64-byte feature reports (no report ID)
REPORT_DESCRIPTOR_BOOT = bytes.fromhex(
    "0601ff"    # Usage Page (Vendor 0xFF01)
    "0901"      # Usage (0x01)
    "a101"      # Collection (Application)
    "0902"      # Usage (0x02)
    "1580"      # Logical Minimum (-128)
    "257f"      # Logical Maximum (127)
    "9540"      # Report Count (64)
    "7508"      # Report Size (8)
    "b102"      # Feature (Data, Var, Abs)
    "c0"        # End Collection
)

# Interface 1: Vendor event + feature (so app can probe 0x8F on MI_01 and get a valid device)
# The updater may open the first matching device (MI_01); if that interface has no feature
# report, SetFeature/GetFeature fail and the device is rejected. We add a 64-byte feature
# report (same as IF2) so probe works on either interface.
REPORT_DESCRIPTOR_IF1 = bytes.fromhex(
    "06ffff"    # Usage Page (Vendor 0xFFFF)
    "0901"      # Usage (0x01)
    "a101"      # Collection (Application)
    "8505"      # Report ID (5) - input (vendor notifications)
    "0901"      # Usage (0x01)
    "1580"      # Logical Minimum (-128)
    "257f"      # Logical Maximum (127)
    "9540"      # Report Count (64)
    "7508"      # Report Size (8)
    "8102"      # Input (Data, Var, Abs)
    "8506"      # Report ID (6) - feature (for 0x8F probe; ID must be non-zero when IDs are used)
    "0902"      # Usage (0x02)
    "1580"      # Logical Minimum (-128)
    "257f"      # Logical Maximum (127)
    "9540"      # Report Count (64)
    "7508"      # Report Size (8)
    "b102"      # Feature (Data, Var, Abs)
    "c0"        # End Collection
)


class UhidCreate2Req(ctypes.Structure):
    _pack_ = 1
    _fields_ = [
        ("name", ctypes.c_ubyte * 128),
        ("phys", ctypes.c_ubyte * 64),
        ("uniq", ctypes.c_ubyte * 64),
        ("rd_size", ctypes.c_uint16),
        ("bus", ctypes.c_uint16),
        ("vendor", ctypes.c_uint32),
        ("product", ctypes.c_uint32),
        ("version", ctypes.c_uint32),
        ("country", ctypes.c_uint32),
        ("rd_data", ctypes.c_ubyte * UHID_DATA_MAX),
    ]


class UhidStartReq(ctypes.Structure):
    _pack_ = 1
    _fields_ = [("dev_flags", ctypes.c_uint64)]


class UhidOutputReq(ctypes.Structure):
    _pack_ = 1
    _fields_ = [
        ("data", ctypes.c_ubyte * UHID_DATA_MAX),
        ("size", ctypes.c_uint16),
        ("rtype", ctypes.c_uint8),
    ]


class UhidGetReportReq(ctypes.Structure):
    _pack_ = 1
    _fields_ = [
        ("id", ctypes.c_uint32),
        ("rnum", ctypes.c_uint8),
        ("rtype", ctypes.c_uint8),
    ]


class UhidGetReportReplyReq(ctypes.Structure):
    _pack_ = 1
    _fields_ = [
        ("id", ctypes.c_uint32),
        ("err", ctypes.c_uint16),
        ("size", ctypes.c_uint16),
        ("data", ctypes.c_ubyte * UHID_DATA_MAX),
    ]


class UhidSetReportReq(ctypes.Structure):
    _pack_ = 1
    _fields_ = [
        ("id", ctypes.c_uint32),
        ("rnum", ctypes.c_uint8),
        ("rtype", ctypes.c_uint8),
        ("size", ctypes.c_uint16),
        ("data", ctypes.c_ubyte * UHID_DATA_MAX),
    ]


class UhidSetReportReplyReq(ctypes.Structure):
    _pack_ = 1
    _fields_ = [
        ("id", ctypes.c_uint32),
        ("err", ctypes.c_uint16),
    ]


class UhidInput2Req(ctypes.Structure):
    _pack_ = 1
    _fields_ = [
        ("size", ctypes.c_uint16),
        ("data", ctypes.c_ubyte * UHID_DATA_MAX),
    ]


class UhidEventUnion(ctypes.Union):
    _pack_ = 1
    _fields_ = [
        ("create2", UhidCreate2Req),
        ("start", UhidStartReq),
        ("output", UhidOutputReq),
        ("get_report", UhidGetReportReq),
        ("get_report_reply", UhidGetReportReplyReq),
        ("set_report", UhidSetReportReq),
        ("set_report_reply", UhidSetReportReplyReq),
        ("input2", UhidInput2Req),
    ]


class UhidEvent(ctypes.Structure):
    _pack_ = 1
    _fields_ = [
        ("type", ctypes.c_uint32),
        ("u", UhidEventUnion),
    ]


@dataclass
class DeviceConfig:
    name: str
    vendor: int
    product: int
    version: int
    device_id: int
    fw_major: int
    fw_minor: int
    fw_version: int | None
    battery: int
    log_dir: Path


def _to_cstr(buf: bytes, max_len: int) -> bytes:
    if len(buf) >= max_len:
        return buf[: max_len - 1]
    return buf


def _hex(data: bytes) -> str:
    return binascii.hexlify(data).decode("ascii")


def build_create2(cfg: DeviceConfig, interface: int) -> UhidEvent:
    """Build a UHID_CREATE2 event for the given interface number.

    interface=1: vendor event/notification device (input reports)
    interface=2: vendor config device (feature reports)
    """
    ev = UhidEvent()
    ev.type = UHID_CREATE2

    name = _to_cstr(cfg.name.encode("ascii", "ignore"), 128)
    phys = _to_cstr(f"uhid/akko/input{interface}".encode("ascii"), 64)
    uniq = _to_cstr(b"dummy", 64)

    if interface == 1:
        rd = REPORT_DESCRIPTOR_IF1
    else:
        rd = REPORT_DESCRIPTOR_IF2

    ctypes.memset(ctypes.byref(ev.u.create2), 0, ctypes.sizeof(UhidCreate2Req))
    ev.u.create2.name[: len(name)] = (ctypes.c_ubyte * len(name)).from_buffer_copy(name)
    ev.u.create2.phys[: len(phys)] = (ctypes.c_ubyte * len(phys)).from_buffer_copy(phys)
    ev.u.create2.uniq[: len(uniq)] = (ctypes.c_ubyte * len(uniq)).from_buffer_copy(uniq)
    ev.u.create2.rd_size = len(rd)
    ev.u.create2.bus = BUS_USB
    ev.u.create2.vendor = cfg.vendor
    ev.u.create2.product = cfg.product
    ev.u.create2.version = cfg.version
    ev.u.create2.country = 0
    ev.u.create2.rd_data[: len(rd)] = (
        ctypes.c_ubyte * len(rd)
    ).from_buffer_copy(rd)
    return ev


def build_create2_boot(cfg: DeviceConfig) -> UhidEvent:
    """Build a UHID_CREATE2 event for the bootloader/DFU device.

    Single interface with usage_page=0xFF01 (bootloader), PID=0x502A.
    """
    ev = UhidEvent()
    ev.type = UHID_CREATE2

    name = _to_cstr(cfg.name.encode("ascii", "ignore"), 128)
    phys = _to_cstr(b"uhid/akko/boot", 64)
    uniq = _to_cstr(b"dummy", 64)
    rd = REPORT_DESCRIPTOR_BOOT

    ctypes.memset(ctypes.byref(ev.u.create2), 0, ctypes.sizeof(UhidCreate2Req))
    ev.u.create2.name[: len(name)] = (ctypes.c_ubyte * len(name)).from_buffer_copy(name)
    ev.u.create2.phys[: len(phys)] = (ctypes.c_ubyte * len(phys)).from_buffer_copy(phys)
    ev.u.create2.uniq[: len(uniq)] = (ctypes.c_ubyte * len(uniq)).from_buffer_copy(uniq)
    ev.u.create2.rd_size = len(rd)
    ev.u.create2.bus = BUS_USB
    ev.u.create2.vendor = cfg.vendor
    ev.u.create2.product = 0x502A  # Bootloader PID
    ev.u.create2.version = cfg.version
    ev.u.create2.country = 0
    ev.u.create2.rd_data[: len(rd)] = (
        ctypes.c_ubyte * len(rd)
    ).from_buffer_copy(rd)
    return ev


def build_set_report_reply(req_id: int, err: int = 0) -> UhidEvent:
    ev = UhidEvent()
    ev.type = UHID_SET_REPORT_REPLY
    ev.u.set_report_reply.id = req_id
    ev.u.set_report_reply.err = err
    return ev


def build_get_report_reply(req_id: int, data: bytes, err: int = 0) -> UhidEvent:
    ev = UhidEvent()
    ev.type = UHID_GET_REPORT_REPLY
    ev.u.get_report_reply.id = req_id
    ev.u.get_report_reply.err = err
    ev.u.get_report_reply.size = len(data)
    ctypes.memset(ctypes.byref(ev.u.get_report_reply.data), 0, UHID_DATA_MAX)
    if data:
        ev.u.get_report_reply.data[: len(data)] = (ctypes.c_ubyte * len(data)).from_buffer_copy(data)
    return ev


def build_input2_report(payload: bytes) -> UhidEvent:
    """Build UHID_INPUT2 event to inject an input report (e.g. vendor notification)."""
    ev = UhidEvent()
    ev.type = UHID_INPUT2
    ev.u.input2.size = len(payload)
    ctypes.memset(ctypes.byref(ev.u.input2.data), 0, UHID_DATA_MAX)
    if payload:
        ev.u.input2.data[: len(payload)] = (ctypes.c_ubyte * len(payload)).from_buffer_copy(payload)
    return ev


def parse_cmd(report: bytes) -> int | None:
    if not report:
        return None
    # Linux feature report usually includes report ID at byte 0
    if report[0] == 0x00 and len(report) >= 2:
        return report[1]
    return report[0]


def build_default_response(cfg: DeviceConfig, last_cmd: int | None, rnum: int) -> bytes:
    # Battery report (dongle style) on report ID 0x05
    if rnum == 0x05 and last_cmd == 0xF7:
        resp = bytearray(65)
        resp[0] = 0x00
        resp[1] = max(1, min(100, cfg.battery))
        resp[2] = 0x00
        resp[3] = 0x00
        resp[4] = 0x01
        resp[5] = 0x01
        resp[6] = 0x01
        return bytes(resp)

    resp = bytearray(65)
    if last_cmd is None:
        return bytes(resp)

    # Feature reports include report ID at byte 0 on Linux.
    # Put cmd at byte 1 so hidapi returns it at index 0.
    resp[0] = rnum
    resp[1] = last_cmd

    version_u16 = cfg.fw_version if cfg.fw_version is not None else ((cfg.fw_major << 8) | cfg.fw_minor)

    if last_cmd == 0x8F:
        # GET_USB_VERSION: device id (u32 LE) + version bytes
        resp[2] = cfg.device_id & 0xFF
        resp[3] = (cfg.device_id >> 8) & 0xFF
        resp[4] = (cfg.device_id >> 16) & 0xFF
        resp[5] = (cfg.device_id >> 24) & 0xFF
        # Driver reads version from resp[7..8] (after hidapi strips report ID)
        resp[8] = version_u16 & 0xFF
        resp[9] = (version_u16 >> 8) & 0xFF
    elif last_cmd == 0x80:
        # GET_REV: simple version bytes
        resp[2] = version_u16 & 0xFF
        resp[3] = (version_u16 >> 8) & 0xFF
    elif last_cmd == 0x84:
        # GET_PROFILE: active profile (0-3), checksum
        resp[2] = 0x00  # profile 0
        resp[8] = 0x7b  # checksum
    elif last_cmd == 0x83:
        # GET_REPORT (polling rate): rate code 0 = 8000Hz, checksum
        resp[8] = 0x7c  # checksum
    elif last_cmd == 0x86:
        # GET_DEBOUNCE: debounce value, checksum
        resp[8] = 0x79  # checksum
    elif last_cmd == 0x87:
        # GET_LEDPARAM: mode, speed, brightness, options, r, g, b
        resp[2] = 0x01  # mode (constant)
        resp[3] = 0x04  # speed
        resp[4] = 0x04  # brightness
        resp[5] = 0x07  # options (normal single color)
        resp[6] = 0xFF  # red
        resp[7] = 0x00  # green
        resp[8] = 0x00  # blue
    elif last_cmd == 0x88:
        # GET_SLEDPARAM: side LED params
        resp[2] = 0x01  # mode
        resp[3] = 0x03  # speed
        resp[4] = 0x04  # brightness
        resp[5] = 0x08  # options (rainbow)
        resp[6] = 0xFF  # r
        resp[7] = 0xFF  # g
        resp[8] = 0xFF  # b
    elif last_cmd == 0x89:
        # GET_KBOPTION: keyboard options, checksum
        resp[8] = 0x76  # checksum
    elif last_cmd == 0x91:
        # GET_SLEEPTIME: sleep/deep sleep timeouts
        resp[8] = 0x6e  # checksum
        # Timeout values (minutes)
        resp[9] = 0x08
        resp[10] = 0x07
        resp[11] = 0x08
        resp[12] = 0x07
        resp[13] = 0x10
        resp[14] = 0x0e
        resp[15] = 0x10
        resp[16] = 0x0e
    elif last_cmd == 0xE6:
        # GET_FEATURE_LIST: minimal bitmap (all zeros = safest default)
        pass
    elif last_cmd == 0xAD:
        # GET_OLED_VERSION: OLED firmware version (u16 LE at byte 7-8), match Windows HAR
        resp[8] = 0x52
        resp[9] = 0x00
    elif last_cmd == 0xAE:
        # GET_MLED_VERSION: matrix LED version (u16 LE at byte 7-8), match Windows HAR
        resp[8] = 0x51
        resp[9] = 0x00
    return bytes(resp)


def parse_transfer_header(payload: bytes) -> tuple[str, int, int] | None:
    if len(payload) < 9:
        return None
    if payload[1] != 0xBA:
        return None
    marker = payload[2]
    if marker not in (0xC0, 0xC2):
        return None
    if marker == 0xC0:
        chunk_count = payload[3] | (payload[4] << 8)
        size = payload[5] | (payload[6] << 8) | (payload[7] << 16)
        return ("start", chunk_count, size)
    # C2 complete header includes checksum/size; keep size if present
    size = 0
    if len(payload) >= 13:
        size = payload[9] | (payload[10] << 8) | (payload[11] << 16) | (payload[12] << 24)
    return ("complete", 0, size)


def main() -> int:
    parser = argparse.ArgumentParser(description="Dummy MonsGeek/Akko UHID device")
    parser.add_argument("--name", default="MonsGeek Dummy Device", help="Device name")
    parser.add_argument("--vid", default="0x3151", help="USB vendor ID (hex)")
    parser.add_argument("--pid", default="0x5030", help="USB product ID (hex)")
    parser.add_argument("--version", default="0x0100", help="USB bcdDevice (hex)")
    parser.add_argument("--device-id", default="0x00000b85", help="Device ID u32 (hex)")
    parser.add_argument("--fw-major", default="0x04", help="FW major/version byte (hex)")
    parser.add_argument("--fw-minor", default="0x07", help="FW minor/version byte (hex)")
    parser.add_argument(
        "--fw-version",
        default=None,
        help="FW version as u16 (decimal or 0xHEX), overrides --fw-major/--fw-minor",
    )
    parser.add_argument("--battery", type=int, default=87, help="Battery percent (1-100)")
    parser.add_argument(
        "--log-dir",
        default="/tmp/monsgeek_dummy",
        help="Directory for logs and reconstructed firmware",
    )
    parser.add_argument("--uhid", default="/dev/uhid", help="UHID device path")
    args = parser.parse_args()

    cfg = DeviceConfig(
        name=args.name,
        vendor=int(args.vid, 16),
        product=int(args.pid, 16),
        version=int(args.version, 16),
        device_id=int(args.device_id, 16),
        fw_major=int(args.fw_major, 16),
        fw_minor=int(args.fw_minor, 16),
        fw_version=int(args.fw_version, 0) if args.fw_version is not None else None,
        battery=args.battery,
        log_dir=Path(args.log_dir),
    )

    # Create two UHID devices: interface 1 (vendor events) and interface 2 (vendor config)
    fd_if2 = os.open(args.uhid, os.O_RDWR | os.O_CLOEXEC)
    os.write(fd_if2, bytes(build_create2(cfg, interface=2)))

    fd_if1 = os.open(args.uhid, os.O_RDWR | os.O_CLOEXEC)
    os.write(fd_if1, bytes(build_create2(cfg, interface=1)))

    fds = {fd_if2: "IF2", fd_if1: "IF1"}

    last_cmd: int | None = None
    last_set: bytes | None = None
    in_transfer = False
    transfer_chunks: list[bytes] = []
    transfer_meta: dict[str, int] = {}

    cfg.log_dir.mkdir(parents=True, exist_ok=True)
    log_path = cfg.log_dir / "uhid_events.log"
    log_fp = log_path.open("a", encoding="utf-8")

    def out(line: str) -> None:
        """Write line to both stdout and log file (with timestamp)."""
        ts = time.strftime("%H:%M:%S")
        full = f"[{ts}] {line}"
        print(full)
        log_fp.write(full + "\n")
        log_fp.flush()

    running = True
    if1_open = False
    if1_fd = None
    last_heartbeat = 0.0
    HEARTBEAT_INTERVAL = 2.0  # seconds between IF1 heartbeat reports

    def _stop(*_args):
        nonlocal running
        running = False

    signal.signal(signal.SIGINT, _stop)
    signal.signal(signal.SIGTERM, _stop)

    out(f"UHID dummy devices created: VID=0x{cfg.vendor:04x} PID=0x{cfg.product:04x}")
    out("  IF1 (vendor events):  phys=uhid/akko/input1  usage_page=0xFFFF usage=0x01")
    out("  IF2 (vendor config):  phys=uhid/akko/input2  usage_page=0xFFFF usage=0x02")
    out("Waiting for host...")
    out(f"Logging to: {log_path}")

    try:
        while running:
            rlist, _, _ = select.select(list(fds.keys()), [], [], 0.5)

            # Send periodic heartbeat on IF1 so Wine's app gets a report
            # even if it opens the HID device after the initial UHID_OPEN report was consumed.
            now = time.monotonic()
            if if1_open and if1_fd is not None and now - last_heartbeat >= HEARTBEAT_INTERVAL:
                try:
                    payload = bytes([5]) + bytes(64)  # Report ID 5, 64 zero bytes
                    os.write(if1_fd, bytes(build_input2_report(payload)))
                    last_heartbeat = now
                except OSError:
                    pass  # device may have been closed

            if not rlist:
                continue
            for fd in rlist:
                tag = fds[fd]
                data = os.read(fd, ctypes.sizeof(UhidEvent))
                if not data:
                    continue
                ev = UhidEvent.from_buffer_copy(data)

                if ev.type == UHID_START:
                    out(f"[{tag}] UHID_START")
                elif ev.type == UHID_OPEN:
                    out(f"[{tag}] UHID_OPEN")
                    # If the updater opens IF1 and blocks on ReadFile, send one input report so it can proceed.
                    if tag == "IF1":
                        if1_open = True
                        if1_fd = fd
                        try:
                            if1_payload = bytes([5]) + bytes(64)  # report ID 5, 64 bytes
                            os.write(fd, bytes(build_input2_report(if1_payload)))
                            last_heartbeat = time.monotonic()
                            out("IF1: sent initial UHID_INPUT2 on OPEN")
                        except OSError as e:
                            out(f"IF1: initial UHID_INPUT2 failed: {e}")
                elif ev.type == UHID_CLOSE:
                    out(f"[{tag}] UHID_CLOSE")
                    if tag == "IF1":
                        if1_open = False
                elif ev.type == UHID_STOP:
                    out(f"[{tag}] UHID_STOP")
                elif ev.type == UHID_OUTPUT:
                    size = ev.u.output.size
                    payload = bytes(ev.u.output.data[:size])
                    out(f"[{tag}] UHID_OUTPUT rtype={ev.u.output.rtype} data={_hex(payload)}")
                elif ev.type == UHID_SET_REPORT:
                    size = ev.u.set_report.size
                    payload = bytes(ev.u.set_report.data[:size])
                    last_set = payload
                    last_cmd = parse_cmd(payload)
                    out(
                        f"[{tag}] SET_REPORT rnum={ev.u.set_report.rnum} rtype={ev.u.set_report.rtype} "
                        f"cmd={cmd_name(last_cmd)} size={size} data={_hex(payload)}"
                    )

                    # Detect ENTER_BOOTLOADER (0x7F with 55AA55AA magic)
                    # payload layout: [report_id=0x00, cmd=0x7F, 0x55, 0xAA, 0x55, 0xAA, ...]
                    if last_cmd == 0x7F and size >= 6 and payload[2:6] == b'\x55\xaa\x55\xaa':
                        out("ENTER_BOOTLOADER detected (magic 55AA55AA) - simulating reboot to DFU...")
                        os.write(fd, bytes(build_set_report_reply(ev.u.set_report.id, err=0)))
                        # Destroy current devices
                        destroy = UhidEvent()
                        destroy.type = UHID_DESTROY
                        for fd_close in list(fds.keys()):
                            try:
                                os.write(fd_close, bytes(destroy))
                            except OSError:
                                pass
                            os.close(fd_close)
                        fds.clear()
                        if1_open = False
                        if1_fd = None
                        out("Devices destroyed, waiting 2s before DFU re-enumeration...")
                        time.sleep(2)
                        # Recreate as bootloader/DFU devices (PID=0x502A, usage_page=0xFF01)
                        boot_cfg = DeviceConfig(
                            name=cfg.name + " (Bootloader)",
                            vendor=cfg.vendor,
                            product=0x502A,  # Bootloader PID
                            version=cfg.version,
                            device_id=cfg.device_id,
                            fw_major=cfg.fw_major,
                            fw_minor=cfg.fw_minor,
                            fw_version=cfg.fw_version,
                            battery=cfg.battery,
                            log_dir=cfg.log_dir,
                        )
                        fd_boot = os.open(args.uhid, os.O_RDWR | os.O_CLOEXEC)
                        os.write(fd_boot, bytes(build_create2_boot(boot_cfg)))
                        fds[fd_boot] = "BOOT"
                        out(f"DFU device created: VID=0x{boot_cfg.vendor:04x} PID=0x{boot_cfg.product:04x} usage_page=0xFF01")
                        last_cmd = None
                        last_set = None
                        in_transfer = False
                        transfer_chunks = []
                        break  # old fds are gone, restart select loop

                    hdr = parse_transfer_header(payload)
                    if hdr:
                        kind, chunk_count, size_bytes = hdr
                        if kind == "start":
                            in_transfer = True
                            transfer_chunks = []
                            transfer_meta = {"chunk_count": chunk_count, "size": size_bytes}
                            out(f"FW_TRANSFER_START chunks={chunk_count} size={size_bytes}")
                            # Send a vendor input report on IF1 so the updater doesn't block on ReadFile.
                            # Report ID 5 + 64 bytes (matches REPORT_DESCRIPTOR_IF1).
                            if tag == "IF2":
                                try:
                                    if1_payload = bytes([5]) + bytes(64)  # report ID 5, padding
                                    os.write(fd_if1, bytes(build_input2_report(if1_payload)))
                                    out("IF1: sent UHID_INPUT2 (vendor notification)")
                                except OSError as e:
                                    out(f"IF1: UHID_INPUT2 failed: {e}")
                        elif kind == "complete":
                            in_transfer = False
                            out_path = cfg.log_dir / "firmware_reconstructed.bin"
                            with out_path.open("wb") as fw:
                                for ch in transfer_chunks:
                                    fw.write(ch)
                            out(f"FW_TRANSFER_COMPLETE wrote={out_path} chunks={len(transfer_chunks)}")
                    elif in_transfer and size >= 2:
                        chunk = payload[1:]
                        transfer_chunks.append(chunk)
                        out(f"FW_CHUNK index={len(transfer_chunks)-1} size={len(chunk)}")
                    os.write(fd, bytes(build_set_report_reply(ev.u.set_report.id, err=0)))
                elif ev.type == UHID_GET_REPORT:
                    rnum = ev.u.get_report.rnum
                    resp = build_default_response(cfg, last_cmd, rnum)
                    out(f"[{tag}] GET_REPORT rnum={rnum} rtype={ev.u.get_report.rtype} cmd={cmd_name(last_cmd)} reply={_hex(resp)}")
                    os.write(fd, bytes(build_get_report_reply(ev.u.get_report.id, resp, err=0)))
                else:
                    out(f"[{tag}] UHID event type={ev.type}")
    finally:
        destroy = UhidEvent()
        destroy.type = UHID_DESTROY
        for fd_close in fds:
            try:
                os.write(fd_close, bytes(destroy))
            except OSError:
                pass
            os.close(fd_close)
        log_fp.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

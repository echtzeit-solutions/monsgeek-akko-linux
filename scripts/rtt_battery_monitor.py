#!/usr/bin/env python3
"""
RTT Battery Monitor — reads 5-byte binary records from BMP serial port.

Each record: [tag:u8] [value:u32 LE]

Usage:
    # Enable RTT on BMP first:
    gdb-multiarch -q \
      -ex 'target extended-remote /dev/ttyACM0' \
      -ex 'monitor swdp_scan' \
      -ex 'attach 1' \
      -ex 'monitor rtt ram 0x20009800 0x20009C00' \
      -ex 'monitor rtt enable' \
      -ex 'detach' -ex 'quit'

    # Then read RTT output:
    python3 rtt_battery_monitor.py /dev/ttyACM1

    # With CSV time-series logging:
    python3 rtt_battery_monitor.py /dev/ttyACM1 --csv battery_log.csv

    # GDB setup (if BMP can't find CB automatically):
    # In GDB: monitor rtt ram 0x20009800 0x20009C00
"""

import argparse
import csv
import struct
import sys
import time

import serial

# Tag schema — must match RTT_TAG_* defines in handlers.c
TAGS = {
    0x01: ("adc_avg",      "u16", lambda v: f"{v & 0xFFFF:5d} (0x{v & 0xFFFF:04X})"),
    0x02: ("batt_raw",     "u8",  lambda v: f"{v & 0xFF:3d}%"),
    0x03: ("batt_level",   "u8",  lambda v: f"{v & 0xFF:3d}%"),
    0x04: ("charger",      "u8",  lambda v: "YES" if (v & 0xFF) else "no"),
    0x05: ("debounce_ctr", "u8",  lambda v: f"{v & 0xFF:3d}"),
    0x10: ("adc_counter",  "u32", lambda v: f"{v:8d}"),
}

# Ordered column names for CSV output
CSV_COLUMNS = ["adc_avg", "batt_raw", "batt_level", "charger", "debounce_ctr", "adc_counter"]
TAG_BY_NAME = {name: tag for tag, (name, _, _) in TAGS.items()}

# Tags that mark the start of a "batch" (first tag emitted per invocation)
BATCH_START_TAG = 0x01  # adc_avg is always first


def format_record(tag: int, val: int) -> str:
    """Format a single RTT record."""
    if tag in TAGS:
        name, _typ, fmt = TAGS[tag]
        return f"  {name:14s} = {fmt(val)}"
    return f"  unknown_{tag:#04x} = {val:#010x}"


def raw_value(tag: int, val: int) -> int | str:
    """Extract the meaningful value for CSV output."""
    _, typ, _ = TAGS[tag]
    if typ == "u8":
        return val & 0xFF
    elif typ == "u16":
        return val & 0xFFFF
    return val


def main():
    parser = argparse.ArgumentParser(description="RTT Battery Monitor for MonsGeek M1 V5")
    parser.add_argument("port", help="BMP RTT serial port (e.g., /dev/ttyACM1)")
    parser.add_argument("-b", "--baud", type=int, default=115200,
                        help="Baud rate (default: 115200)")
    parser.add_argument("--raw", action="store_true",
                        help="Print each record individually instead of batched")
    parser.add_argument("--csv", metavar="FILE", dest="csv_file",
                        help="Log time-series data to CSV file")
    parser.add_argument("--quiet", "-q", action="store_true",
                        help="Suppress terminal output (only write CSV)")
    args = parser.parse_args()

    try:
        ser = serial.Serial(args.port, args.baud, timeout=0.1)
    except serial.SerialException as e:
        print(f"Error opening {args.port}: {e}", file=sys.stderr)
        sys.exit(1)

    csv_writer = None
    csv_fh = None
    if args.csv_file:
        csv_fh = open(args.csv_file, "w", newline="")
        csv_writer = csv.writer(csv_fh)
        csv_writer.writerow(["timestamp", "elapsed_s"] + CSV_COLUMNS)
        print(f"Logging to {args.csv_file}", file=sys.stderr)

    if not args.quiet:
        print(f"RTT Battery Monitor — reading from {args.port}")
        print(f"Waiting for data... (tags: {', '.join(t[0] for t in TAGS.values())})")
        print()

    buf = bytearray()
    batch: dict[int, int] = {}
    batch_count = 0
    t0 = time.monotonic()

    try:
        while True:
            data = ser.read(256)
            if not data:
                continue

            buf.extend(data)

            # Process complete 5-byte records
            while len(buf) >= 5:
                tag = buf[0]
                val = struct.unpack_from('<I', buf, 1)[0]

                # Validate tag — if unknown, try to resync by skipping a byte
                if tag not in TAGS:
                    buf.pop(0)
                    continue

                buf = buf[5:]

                if args.raw:
                    if not args.quiet:
                        print(format_record(tag, val))
                    continue

                # Batch mode: collect all tags, print on batch boundary
                if tag == BATCH_START_TAG and batch:
                    # Emit previous batch
                    batch_count += 1
                    now = time.time()
                    elapsed = time.monotonic() - t0

                    if not args.quiet:
                        ts = time.strftime("%H:%M:%S", time.localtime(now))
                        print(f"[{ts}] #{batch_count}  (t={elapsed:.1f}s)")
                        for t in sorted(batch.keys()):
                            print(format_record(t, batch[t]))
                        print()

                    if csv_writer:
                        row = [
                            f"{now:.3f}",
                            f"{elapsed:.2f}",
                        ]
                        for col_name in CSV_COLUMNS:
                            t_id = TAG_BY_NAME[col_name]
                            if t_id in batch:
                                row.append(raw_value(t_id, batch[t_id]))
                            else:
                                row.append("")
                        csv_writer.writerow(row)
                        csv_fh.flush()

                    batch.clear()

                batch[tag] = val

    except KeyboardInterrupt:
        if not args.quiet:
            print(f"\nStopped after {batch_count} batches, "
                  f"{time.monotonic() - t0:.1f}s elapsed.")
    finally:
        ser.close()
        if csv_fh:
            csv_fh.close()
            print(f"CSV written: {args.csv_file} ({batch_count} rows)",
                  file=sys.stderr)


if __name__ == "__main__":
    main()

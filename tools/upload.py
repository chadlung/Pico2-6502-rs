#!/usr/bin/env python3
"""Upload a 6502 program to the Pico 6502 computer over USB serial and run it.

examples:
  upload.py hello.bin                  load at $0200 and run
  upload.py game.bin --addr 8000       load at $8000 and run
  upload.py rom.bin --addr 0 --no-run  load a full memory image, don't run
  upload.py hello.prg                  load address from the 2-byte .prg header

Requires pyserial (pip install pyserial).
"""
import argparse
import binascii
import sys
import time

import serial
from serial.tools import list_ports

IO_START, IO_END = 0xF000, 0xF100   # I/O page; loads must skip it
RASPBERRY_PI_VID = 0x2E8A
PICO_SDK_CDC_PIDS = (0x0009, 0x000A)   # Pico SDK USB serial on RP2350 and RP2040


def hex_addr(text):
    value = int(text.lstrip("$"), 16)
    if not 0 <= value <= 0xFFFF:
        raise argparse.ArgumentTypeError("address must be 0000-FFFF")
    return value


def find_pico():
    for port in list_ports.comports():
        # Match the product ID too, so a Debug Probe's serial port is not picked.
        if port.vid == RASPBERRY_PI_VID and port.pid in PICO_SDK_CDC_PIDS:
            return port.device
    return None


def wait_for(ser, prefixes, timeout):
    """Read lines until one starts with one of prefixes and return it."""
    deadline = time.monotonic() + timeout
    pending = b""
    while time.monotonic() < deadline:
        pending += ser.read(ser.in_waiting or 1)
        while b"\n" in pending:
            raw, pending = pending.split(b"\n", 1)
            text = raw.decode("ascii", "replace").strip()
            if text.startswith(prefixes):
                return text
    sys.exit(f"error: no {' or '.join(prefixes)} reply from the Pico")


def segments(addr, data):
    """Split the image around the I/O page, which cannot be loaded."""
    end = addr + len(data)
    if end <= IO_START or addr >= IO_END:
        return [(addr, data)]
    skipped = data[max(addr, IO_START) - addr:min(end, IO_END) - addr]
    if any(skipped):
        print("warning: image has data in the I/O page $F000-$F0FF; not loaded",
              file=sys.stderr)
    parts = []
    if addr < IO_START:
        parts.append((addr, data[:IO_START - addr]))
    if end > IO_END:
        parts.append((IO_END, data[IO_END - addr:]))
    return parts


def load(ser, addr, data):
    crc = binascii.crc_hqx(data, 0xFFFF)
    ser.write(f"l {addr:04X} {len(data):X} {crc:04X}\n".encode())
    reply = wait_for(ser, ("READY", "ERR"), 3)
    if reply.startswith("ERR"):
        sys.exit(f"error: {reply}")
    ser.write(data)
    ser.flush()
    reply = wait_for(ser, ("OK", "ERR"), 10)
    if reply.startswith("ERR"):
        sys.exit(f"error: {reply}")
    print(reply)


def main():
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("file", help="raw .bin, or .prg with a load-address header")
    parser.add_argument("-a", "--addr", type=hex_addr,
                        help="load address in hex (default 0200, or the .prg header)")
    parser.add_argument("-r", "--run", type=hex_addr, metavar="ADDR",
                        help="start address (default: the load address, or the "
                             "image's own reset vector if it includes $FFFC-$FFFD)")
    parser.add_argument("-n", "--no-run", action="store_true", help="load only")
    parser.add_argument("-p", "--port", help="serial port (default: find the Pico)")
    args = parser.parse_args()

    with open(args.file, "rb") as f:
        data = f.read()
    addr = args.addr
    if args.file.lower().endswith(".prg"):
        if len(data) < 2:
            sys.exit("error: .prg file too short")
        if addr is None:
            addr = data[0] | data[1] << 8
        data = data[2:]
    if addr is None:
        addr = 0x0200
    if not data:
        sys.exit("error: nothing to load")
    if addr + len(data) > 0x10000:
        sys.exit(f"error: {len(data)} bytes at ${addr:04X} runs past $FFFF")

    port = args.port or find_pico()
    if not port:
        sys.exit("error: no Pico found; give the port with --port")

    with serial.Serial(port, 115200, timeout=0.1) as ser:
        ser.write(b"\x03\n")          # stop any running program, fresh prompt
        time.sleep(0.3)
        ser.reset_input_buffer()

        for seg_addr, seg_data in segments(addr, data):
            load(ser, seg_addr, seg_data)
        if args.no_run:
            return

        if args.run is not None:
            command = f"g {args.run:04X}"
        elif addr <= 0xFFFC and addr + len(data) >= 0xFFFE:
            command = "g"
        else:
            command = f"g {addr:04X}"
        ser.write(f"{command}\n".encode())
        reply = wait_for(ser, ("OK", "ERR"), 3)
        if reply.startswith("ERR"):
            sys.exit(f"error: {reply}")
        print(reply)


if __name__ == "__main__":
    main()

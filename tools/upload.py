#!/usr/bin/env python3
"""Upload a raw 6502 binary or Commodore PRG over the USB monitor."""

import argparse
import binascii
import sys
import time

import serial
from serial.tools import list_ports

IO_START, IO_END = 0xF000, 0xF100
RASPBERRY_PI_VID = 0x2E8A
PICO_SERIAL_PID = 0x0009


def hex_address(text):
    """Parse a 16-bit hexadecimal address."""
    value = int(text.lstrip("$"), 16)
    if not 0 <= value <= 0xFFFF:
        raise argparse.ArgumentTypeError("address must be 0000-FFFF")
    return value


def find_pico():
    """Find this firmware's USB CDC port."""
    for port in list_ports.comports():
        if port.vid == RASPBERRY_PI_VID and port.pid == PICO_SERIAL_PID:
            return port.device
    return None


def wait_for(port, prefixes, timeout):
    """Read until a line begins with one of the expected prefixes."""
    deadline = time.monotonic() + timeout
    pending = b""
    while time.monotonic() < deadline:
        pending += port.read(port.in_waiting or 1)
        while b"\n" in pending:
            raw, pending = pending.split(b"\n", 1)
            text = raw.decode("ascii", "replace").strip()
            if text.startswith(prefixes):
                return text
    sys.exit(f"error: no {' or '.join(prefixes)} reply from the Pico")


def segments(address, data):
    """Split an image around the memory-mapped I/O page."""
    end = address + len(data)
    if end <= IO_START or address >= IO_END:
        return [(address, data)]
    skipped = data[max(address, IO_START) - address : min(end, IO_END) - address]
    if any(skipped):
        print("warning: data in $F000-$F0FF was not loaded", file=sys.stderr)
    result = []
    if address < IO_START:
        result.append((address, data[: IO_START - address]))
    if end > IO_END:
        result.append((IO_END, data[IO_END - address :]))
    return result


def load(port, address, data):
    """Load one contiguous byte range with CRC-16 verification."""
    crc = binascii.crc_hqx(data, 0xFFFF)
    port.write(f"l {address:04X} {len(data):X} {crc:04X}\n".encode())
    reply = wait_for(port, ("READY", "ERR"), 3)
    if reply.startswith("ERR"):
        sys.exit(f"error: {reply}")
    port.write(data)
    port.flush()
    reply = wait_for(port, ("OK", "ERR"), 10)
    if reply.startswith("ERR"):
        sys.exit(f"error: {reply}")
    print(reply)


def main():
    """Command-line entry point."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", help="raw .bin or .prg with a two-byte load address")
    parser.add_argument("-a", "--addr", type=hex_address)
    parser.add_argument("-r", "--run", type=hex_address, metavar="ADDR")
    parser.add_argument("-n", "--no-run", action="store_true")
    parser.add_argument("-p", "--port")
    args = parser.parse_args()

    with open(args.file, "rb") as source:
        data = source.read()
    address = args.addr
    if args.file.lower().endswith(".prg"):
        if len(data) < 2:
            sys.exit("error: .prg file too short")
        address = address if address is not None else data[0] | data[1] << 8
        data = data[2:]
    address = 0x0200 if address is None else address
    if not data or address + len(data) > 0x10000:
        sys.exit("error: image is empty or runs past $FFFF")

    device = args.port or find_pico()
    if not device:
        sys.exit("error: no Pico found; give the port with --port")
    with serial.Serial(device, 115200, timeout=0.1) as port:
        port.write(b"\x03\n")
        time.sleep(0.3)
        port.reset_input_buffer()
        for start, block in segments(address, data):
            load(port, start, block)
        if args.no_run:
            return
        if args.run is not None:
            command = f"g {args.run:04X}"
        elif address <= 0xFFFC and address + len(data) >= 0xFFFE:
            command = "g"
        else:
            command = f"g {address:04X}"
        port.write(f"{command}\n".encode())
        print(wait_for(port, ("OK", "ERR"), 3))


if __name__ == "__main__":
    main()


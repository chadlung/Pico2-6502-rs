#!/usr/bin/env python3
"""Exercise the Pico 6502 firmware (or the simulator's -p port) over serial.

Uploads the example programs and checks the monitor's replies: loading and
checksums, running and stopping, serial I/O, breakpoints, single-stepping, the
blackjack example, the LCD text mirror, the speed limit and, when make test
in host/ has downloaded it, the Klaus Dormann 6502 functional test.  Leaves
the hello demo running.

Needs pyserial and the assembled asm/hello.bin, asm/echo.bin and
asm/blackjack.bin.

examples:
  selftest.py                       find the Pico by its USB ID
  selftest.py --expect-lcd          also fail if no display is detected
  selftest.py --port /dev/pts/3     test the simulator (sim6502 -p)
"""
import argparse
import os
import re
import subprocess
import sys
import time

import serial

from upload import find_pico

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
UPLOAD = os.path.join(ROOT, "tools", "upload.py")
HELLO = os.path.join(ROOT, "asm", "hello.bin")
ECHO = os.path.join(ROOT, "asm", "echo.bin")
BLACKJACK = os.path.join(ROOT, "asm", "blackjack.bin")
KLAUS = os.path.join(ROOT, "host", "fake6502-upstream", "tests", "6502_functional_test.bin")

failures = 0


def check(name, ok, output=""):
    global failures
    print(f"{'PASS' if ok else 'FAIL'}  {name}")
    if not ok:
        failures += 1
        print("      " + output.strip().replace("\n", "\n      "))


def talk(ser, data, wait=0.5):
    """Send data, give the monitor time to answer, return everything received."""
    ser.write(data.encode() if isinstance(data, str) else data)
    time.sleep(wait)
    return ser.read(1 << 20).decode("ascii", "replace")


def read_until(ser, text, timeout):
    out = ""
    deadline = time.monotonic() + timeout
    while text not in out and time.monotonic() < deadline:
        out += ser.read(4096).decode("ascii", "replace")
    return out


def upload(port, *args):
    result = subprocess.run([sys.executable, UPLOAD, "--port", port, *args],
                            capture_output=True, text=True, timeout=180)
    return result.returncode == 0, result.stdout + result.stderr


def measure_khz(ser, limit):
    """Run a counting loop for two seconds under a speed limit; return kHz."""
    talk(ser, f"t {limit}\nw 0300 E8 4C 00 03\ng 300\n", 2.0)   # INX / JMP $0300
    out = talk(ser, b"\x03")
    match = re.search(r"\((\d+) kHz\)", out)
    return (int(match.group(1)) if match else None), out


def main():
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("-p", "--port", help="serial port (default: find the Pico)")
    parser.add_argument("--expect-lcd", action="store_true",
                        help="fail if the firmware did not detect a display")
    args = parser.parse_args()

    for path in (HELLO, ECHO, BLACKJACK):
        if not os.path.exists(path):
            sys.exit(f"error: {os.path.relpath(path, ROOT)} missing; run make in asm/")
    port = args.port or find_pico()
    if not port:
        sys.exit("error: no Pico found; give the port with --port")
    print(f"Testing {port}\n")

    with serial.Serial(port, 115200, timeout=0.1) as ser:
        out = talk(ser, b"\x03\nb -\nt 1000\n")
        check("monitor answers", "Speed limit 1000 kHz" in out, out)

    ok, out = upload(port, HELLO)
    check("upload and run hello.bin", ok and "OK running from $0200" in out, out)

    with serial.Serial(port, 115200, timeout=0.1) as ser:
        out = talk(ser, "d\n")
        check("LCD text shows hello", "|RASPBERRY PICO 2|" in out and "|HELLO FROM 6502 |" in out, out)
        found = re.search(r"display at I2C address ([0-9A-F]{2})", out)
        print(f"INFO  display: {'I2C address 0x' + found.group(1) if found else 'not detected'}")
        if args.expect_lcd:
            check("display detected", found is not None, out)

        out = talk(ser, "w F000 01\n" + "".join(f"w F001 {0x41 + i:02X}\n" for i in range(20)) + "d\n", 1.0)
        check("text wraps to the next row",
              "|ABCDEFGHIJKLMNOP|" in out and "|QRST            |" in out and "row 1 col 4" in out, out)

        out = talk(ser, "l 0300 2 0000\n", 0.3) + talk(ser, b"\x01\x02")
        check("bad checksum rejected", "ERR checksum" in out, out)

        out = talk(ser, "l EFFF 2 0\n")
        check("load into I/O page rejected", "ERR load overlaps" in out, out)

        out = talk(ser, "w 0300 00\ng 300\n")
        check("BRK without a vector stops", "BRK (no IRQ/BRK vector set) at $0300" in out, out)

        out = talk(ser, "m 0200 20\n")
        check("memory dump", "0200: 78 D8 A2 FF 9A A9 01 8D 00 F0" in out, out)

    ok, out = upload(port, ECHO)
    check("upload and run echo.bin", ok and "OK loaded 31 bytes at $0200-$021E" in out, out)

    with serial.Serial(port, 115200, timeout=0.1) as ser:
        out = talk(ser, "Hi\r\nthere", 1.0)
        check("program reads serial input and echoes it", "Hi" in out and "there" in out, repr(out))
        out = talk(ser, b"\x03") + talk(ser, "d\n")
        check("Ctrl-C stops the program", "Stopped at $02" in out, out)
        check("typed text on the LCD", "|Hi              |" in out and "|there           |" in out, out)

    ok, out = upload(port, BLACKJACK)
    check("upload and run blackjack.bin", ok and "OK running from $0200" in out, out)

    with serial.Serial(port, 115200, timeout=0.1) as ser:
        # Seed $1234 with the fixed-seed flag set: the first hand is always
        # the dealer's 8 5 A 9 (bust) against the player's J Q.
        talk(ser, b"\x03")
        talk(ser, "w 00F0 34 12 01\ng 200\n")
        out = talk(ser, " ", 1.0) + talk(ser, "S", 3.0)
        out += talk(ser, b"\x03") + talk(ser, "d\nw 00F2 00\n")
        check("blackjack plays a seeded hand",
              "Dealer busts. You win!" in out and "|D:85A9      BUST|" in out
              and "|P:JQ         WIN|" in out, out)

    ok, out = upload(port, HELLO, "--no-run")
    check("upload without running", ok and "OK loaded" in out and "running" not in out, out)

    with serial.Serial(port, 115200, timeout=0.1) as ser:
        out = talk(ser, "u 0200 3\nu\n")
        check("disassemble with u",
              "0200  78        SEI" in out and "0202  A2 FF     LDX #$FF" in out
              and "0207  8D 00 F0  STA $F000  ; LCD_CONTROL" in out, out)
        out = talk(ser, "b 0225\ng 200\n")
        check("breakpoint", "Breakpoint at $0225" in out and "85 FB     STA $FB" in out, out)
        out = talk(ser, "c\n")
        check("continue to the breakpoint again", "Breakpoint at $0225" in out, out)
        out = talk(ser, "b -\nc\n")
        check("continue to the end", "Halted in endless loop at $0222" in out, out)
        out = talk(ser, "x\ns 3\n")
        check("reset and single-step", "PC=0202" in out and "PC=0204" in out, out)

        khz, out = measure_khz(ser, 1000)
        check("1 MHz speed limit within 10%", khz is not None and 900 <= khz <= 1100, out)
        print(f"INFO  limited speed: {khz} kHz")
        khz, out = measure_khz(ser, 0)
        check("unlimited speed runs", khz is not None, out)
        print(f"INFO  unlimited speed: {khz} kHz")

    if os.path.exists(KLAUS):
        with serial.Serial(port, 115200, timeout=0.1) as ser:
            talk(ser, "t 0\n")
        start = time.monotonic()
        ok, out = upload(port, KLAUS, "--addr", "0")
        check("upload 64K memory image", ok and out.count("OK loaded") == 2
              and "OK running from $0400" in out, out)
        print(f"INFO  64K upload took {time.monotonic() - start:.1f} s")
        with serial.Serial(port, 115200, timeout=0.1) as ser:
            out = read_until(ser, "Halted", 600)
            check("Klaus Dormann functional test", "Halted in endless loop at $3469" in out, out)
            for line in out.splitlines():
                if "Halted" in line:
                    print(f"INFO  {line.strip()}")
    else:
        print("SKIP  Klaus Dormann functional test (run make test in host/ to download it)")

    with serial.Serial(port, 115200, timeout=0.1) as ser:
        talk(ser, "t 1000\n")
    upload(port, HELLO)

    print(f"\n{'All tests passed.' if failures == 0 else f'{failures} test(s) FAILED.'}")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Check the disassembler (src/disasm.rs) against 64tass.

Every line the disassembler decodes is assembled again with 64tass; the bytes
must be the opcode and operand it came from.  Exactly the 105 undocumented
opcodes must show as ???, and a few fixed cases check register names and
branch targets.  Run through `make test-disasm`.
"""
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
ASSEMBLER = os.environ.get("TASS", "64tass")

EXPECTED_EXTRAS = [
    "0207 3 8D 00 F0  STA $F000  ; LCD_CONTROL",
    "050B 2 D0 F5     BNE $0502",
    "0300 3 BD 10 F0  LDA $F010,X  ; SERIAL_DATA",
    "FFFE 2 10 01     BPL $0001",
]

lines = subprocess.run(["cargo", "run", "--quiet", "--release", "--example", "disasm_dump"],
                       cwd=ROOT,
                       capture_output=True, text=True, check=True).stdout.splitlines()
table, extras = lines[:256], lines[256:]

failures = 0
decoded = []
unknown = 0
for op, line in enumerate(table):
    _, length, text = line.split(" ", 2)
    instruction = text[10:].split("  ;")[0]
    if instruction == "???":
        unknown += 1
        if length != "1":
            print(f"FAIL {op:02X}: ??? should have length 1")
            failures += 1
    else:
        decoded.append((op, int(length), instruction))

if len(decoded) != 151 or unknown != 105:
    print(f"FAIL expected 151 documented and 105 unknown opcodes, got {len(decoded)} and {unknown}")
    failures += 1

source = ['        .cpu "6502"', "        * = $0000"]
for _, _, instruction in decoded:
    source += ["        .logical $1000", f"        {instruction}", "        .endlogical"]

with tempfile.TemporaryDirectory() as tmp:
    asm, binary = os.path.join(tmp, "all.asm"), os.path.join(tmp, "all.bin")
    with open(asm, "w") as f:
        f.write("\n".join(source) + "\n")
    result = subprocess.run([ASSEMBLER, "-q", "-a", "--m6502", "--nostart", "-o", binary, asm],
                            capture_output=True, text=True)
    if result.returncode != 0:
        print(result.stdout + result.stderr)
        sys.exit("FAIL: 64tass could not assemble the disassembly")
    with open(binary, "rb") as f:
        assembled = f.read()

offset = 0
for op, length, instruction in decoded:
    want = bytes([op, 0x34, 0x12][:length])
    got = assembled[offset:offset + length]
    if got != want:
        print(f"FAIL {op:02X}: '{instruction}' assembles to {got.hex(' ').upper()}")
        failures += 1
    offset += length
if offset != len(assembled):
    print(f"FAIL assembled {len(assembled)} bytes, expected {offset}")
    failures += 1

for want, got in zip(EXPECTED_EXTRAS, extras):
    if got != want:
        print(f"FAIL expected '{want}', got '{got}'")
        failures += 1

if failures:
    sys.exit(f"{failures} disassembler check(s) failed")
print(f"Disassembler OK: {len(decoded)} opcodes match 64tass, {unknown} show as ???")

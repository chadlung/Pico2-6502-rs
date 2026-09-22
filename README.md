# Pico2-6502-rs

A small MOS 6502 computer inside a Raspberry Pi Pico 2, implemented in Rust.
It reproduces the architecture of [Pico2-6502](https://github.com/chadlung/Pico2-6502):

```text
6502 assembly -> binary -> USB monitor -> NMOS 6502 -> $F000 I/O -> I2C LCD1602
```

The project uses [`mos6502` 0.10.1](https://crates.io/crates/mos6502) for the
CPU. It was selected over `cpu6502` because it is published, `no_std`, supports
NMOS decimal mode and undocumented opcodes, exposes an embedded-friendly bus,
and is validated with Klaus Dormann's functional suite. The portable machine
core, desktop simulator, monitor, RP2350 firmware, USB CDC front end, and
PCF8574/HD44780 driver are Rust.

## What works

- 64 KiB address space with memory-mapped I/O at `$F000-$F0FF`
- NMOS 6502 instruction stepping, registers, cycle counts, reset and vectors
- 16x2 LCD text mirror, cursor, clear/home and backlight registers
- USB serial receive/transmit registers
- Monitor commands for memory, writes, run/continue, stepping, disassembly,
  breakpoints, speed, LCD display, reset and CRC-checked binary upload
- Built-in `RASPBERRY PICO 2 / HELLO FROM 6502` demo
- Desktop simulator using the exact same CPU, bus and monitor core
- `no_std` Pico 2 firmware using Embassy USB and blocking I2C
- Raw `.bin` and `.prg` uploader

The monitor's `i` command currently reports that display probing happens at
boot; unlike the C project, it does not yet perform the detailed live GPIO
line diagnosis.

## Memory map

| Address | Access | Function |
|---|---:|---|
| `$F000` | W | LCD control: bit 0 clear, bit 1 home |
| `$F001` | W | LCD character data |
| `$F002` | R/W | LCD row (0-1) |
| `$F003` | R/W | LCD column (0-15) |
| `$F004` | R/W | LCD backlight |
| `$F010` | R/W | Serial data |
| `$F011` | R | Serial ready in bit 7 |

## Hardware

The wiring matches Pico2-6502:

| Pico 2 | Freenove I2C LCD1602 |
|---|---|
| VBUS, physical pin 40 | VCC |
| GND, physical pin 38 | GND |
| GP4, physical pin 6 | SDA |
| GP5, physical pin 7 | SCL |

The LCD is powered from 5 V. This relies on the RP2350's powered 5 V-tolerant
GPIO; do not copy this wiring to an original RP2040 Pico without a level
shifter. The driver scans the PCF8574T and PCF8574AT address ranges used by
these backpacks.

## Desktop simulator

```sh
cargo run --bin sim6502
cargo test
```

The simulator accepts the same line-oriented monitor commands on standard
input. Terminal input is canonical, so press Enter after commands.

## Build the Pico 2 firmware

Install stable Rust and the Cortex-M33 target:

```sh
rustup target add thumbv8m.main-none-eabihf
cargo build --release --no-default-features --features firmware \
  --bin pico6502 --target thumbv8m.main-none-eabihf
```

The ELF is written to:

```text
target/thumbv8m.main-none-eabihf/release/pico6502
```

Flash the ELF with current `picotool`:

```sh
picotool load -u -v -x -t elf \
  target/thumbv8m.main-none-eabihf/release/pico6502
```

Or convert it to UF2 for drag-and-drop flashing:

```sh
picotool uf2 convert -t elf \
  target/thumbv8m.main-none-eabihf/release/pico6502 pico6502.uf2
```

## Monitor and uploads

Connect with a serial terminal at any nominal baud rate and type `h`. Example:

```text
m 0200 40
w 0300 A9 41 8D 01 F0 4C 05 03
g 0300
```

Assemble the examples with 64tass and upload one with pyserial:

```sh
make -C asm
python3 -m pip install pyserial
python3 tools/upload.py asm/echo.bin
```

## Project layout

| Path | Purpose |
|---|---|
| `src/bus.rs` | RAM, `$F000` I/O, LCD mirror and device queues |
| `src/machine.rs` | `mos6502` orchestration and stepping |
| `src/monitor.rs` | Portable monitor and binary loader |
| `src/disasm.rs` | NMOS-aware monitor disassembly |
| `src/bin/sim6502.rs` | Desktop simulator |
| `src/bin/pico6502.rs` | Pico 2 USB/I2C firmware |
| `asm/` | 64tass example programs and register definitions |
| `tools/upload.py` | CRC-checked USB uploader |
| `tests/` | Host-side behavior tests |

## License and attribution

BSD-2-Clause. The memory map, demo bytes, examples, uploader protocol, and
hardware behavior are derived from Chad Lung's BSD-2-Clause Pico2-6502 project.
The `mos6502` dependency is BSD-3-Clause; Embassy crates are MIT OR
Apache-2.0. See dependency sources for their license texts.


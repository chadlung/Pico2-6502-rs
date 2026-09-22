//! USB/terminal monitor shared by the simulator and firmware.

use core::fmt::{self, Write};

use heapless::{Deque, String};

use crate::bus::IO_PAGE;
use crate::disasm::disassemble;
use crate::machine::Machine;

const CTRL_C: u8 = 0x03;
const LINE_LEN: usize = 80;

/// Why a running program stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// User pressed Ctrl-C.
    User,
    /// Program counter reached a breakpoint.
    Breakpoint,
    /// Program reached a `BRK` without an interrupt vector.
    UnhandledBrk,
    /// Program counter did not change after an instruction.
    EndlessLoop,
    /// CPU executed a stop/JAM instruction.
    CpuStopped,
}

/// Line-oriented machine monitor with a bounded output queue.
pub struct Monitor {
    line: String<LINE_LEN>,
    output: Deque<u8, 8192>,
    running: bool,
    breakpoint: Option<u16>,
    skip_breakpoint: bool,
    speed_khz: u32,
    run_cycles: u64,
    last_was_cr: bool,
    loading: Option<Loader>,
}

#[derive(Clone, Copy, Debug)]
struct Loader {
    start: u16,
    cursor: u16,
    remaining: u32,
    expected_crc: u16,
    crc: u16,
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Monitor {
    /// Creates an idle monitor with a 1 MHz speed target.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            line: String::new(),
            output: Deque::new(),
            running: false,
            breakpoint: None,
            skip_breakpoint: false,
            speed_khz: 1000,
            run_cycles: 0,
            last_was_cr: false,
            loading: None,
        }
    }

    /// Writes the startup banner and prompt.
    pub fn init(&mut self, machine: &Machine) {
        let display = if machine.cpu.memory.display_address().is_some() {
            ""
        } else {
            " (display not detected)"
        };
        self.print(format_args!(
            "\r\nPico 6502: mos6502 CPU, 64K RAM, 16x2 LCD{display}\r\nType h for help.\r\n> "
        ));
    }

    /// Returns whether a program is currently running.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.running
    }

    /// Configured emulation speed in kHz; zero means unlimited.
    #[must_use]
    pub const fn speed_khz(&self) -> u32 {
        self.speed_khz
    }

    /// Resets the machine at `address` and begins execution.
    pub fn start_at(&mut self, machine: &mut Machine, address: u16) {
        machine.reset_at(address);
        self.run_cycles = 0;
        self.running = true;
        self.skip_breakpoint = false;
        self.print(format_args!(
            "OK running from ${address:04X} (Ctrl-C stops)\r\n"
        ));
    }

    /// Removes one byte waiting to be sent to the terminal.
    pub fn pop_output(&mut self) -> Option<u8> {
        self.output.pop_front()
    }

    /// Processes one byte received from the terminal.
    pub fn input(&mut self, machine: &mut Machine, byte: u8) {
        if self.feed_loader(machine, byte) {
            return;
        }

        let lf_after_cr = byte == b'\n' && self.last_was_cr;
        self.last_was_cr = byte == b'\r';
        if lf_after_cr {
            return;
        }

        if self.running {
            if byte == CTRL_C {
                self.stop(machine, StopReason::User);
            } else {
                let _ = machine.cpu.memory.push_serial_rx(byte);
            }
            return;
        }

        match byte {
            b'\r' | b'\n' => {
                self.print(format_args!("\r\n"));
                let mut command = String::<LINE_LEN>::new();
                core::mem::swap(&mut command, &mut self.line);
                self.execute(machine, command.as_str());
                if !self.running && self.loading.is_none() {
                    self.print(format_args!("> "));
                }
            }
            CTRL_C => {
                self.line.clear();
                self.print(format_args!("^C\r\n> "));
            }
            0x08 | 0x7F if !self.line.is_empty() => {
                self.line.pop();
                self.print(format_args!("\x08 \x08"));
            }
            0x20..=0x7E if self.line.len() < LINE_LEN => {
                let _ = self.line.push(char::from(byte));
                let _ = self.output.push_back(byte);
            }
            _ => {}
        }
    }

    /// Executes one instruction when running and reports an automatic stop.
    pub fn tick(&mut self, machine: &mut Machine) -> u64 {
        if !self.running {
            return 0;
        }
        let pc = machine.cpu.registers.program_counter;
        if self.breakpoint == Some(pc) && !self.skip_breakpoint {
            self.stop(machine, StopReason::Breakpoint);
            return 0;
        }
        self.skip_breakpoint = false;
        if machine.has_unhandled_brk() {
            self.stop(machine, StopReason::UnhandledBrk);
            return 0;
        }
        let step = machine.step();
        self.run_cycles = self.run_cycles.wrapping_add(step.cycles);
        if machine.is_stopped() {
            self.stop(machine, StopReason::CpuStopped);
        } else if step.old_pc == step.new_pc {
            self.stop(machine, StopReason::EndlessLoop);
        }
        step.cycles
    }

    fn execute(&mut self, machine: &mut Machine, line: &str) {
        let mut words = line.split_ascii_whitespace();
        let Some(command) = words.next() else { return };
        match command.as_bytes()[0].to_ascii_lowercase() {
            b'h' | b'?' => self.help(),
            b'r' => self.registers(machine),
            b'm' => self.memory(machine, &mut words),
            b'w' => self.write_memory(machine, &mut words),
            b'g' => self.go(machine, &mut words),
            b'c' => {
                self.skip_breakpoint = true;
                self.running = true;
                self.run_cycles = 0;
                self.print(format_args!(
                    "OK continuing at ${:04X} (Ctrl-C stops)\r\n",
                    machine.cpu.registers.program_counter
                ));
            }
            b's' => self.single_step(machine, &mut words),
            b'u' => self.unassemble(machine, &mut words),
            b'x' => {
                machine.reset();
                self.registers(machine);
            }
            b'b' => self.breakpoint(&mut words),
            b't' => self.speed(&mut words),
            b'd' => self.display(machine),
            b'i' => self.print(format_args!(
                "INFO use the firmware boot scan to reprobe the display\r\n"
            )),
            b'l' => self.load(&mut words),
            _ => self.print(format_args!("ERR unknown command, h for help\r\n")),
        }
    }

    fn help(&mut self) {
        self.print(format_args!(
            "Numbers are hex unless noted.\r\n  r                 show CPU registers\r\n  m ADDR [LEN]      dump memory\r\n  w ADDR BB [BB..]  write bytes\r\n  g [ADDR]          reset CPU and run\r\n  c                 continue\r\n  s [N]             single-step\r\n  u [ADDR [N]]      disassemble\r\n  x                 reset without running\r\n  b [ADDR | -]      breakpoint\r\n  t [KHZ]           speed limit\r\n  d                 show LCD mirror\r\n  l ADDR LEN CRC    load raw bytes\r\n  Ctrl-C            stop\r\n"
        ));
    }

    fn registers(&mut self, machine: &Machine) {
        let r = machine.cpu.registers;
        self.print(format_args!(
            "PC={:04X} A={:02X} X={:02X} Y={:02X} SP={:02X} P={:08b}  ",
            r.program_counter,
            r.accumulator,
            r.index_x,
            r.index_y,
            r.stack_pointer.0,
            r.status.bits()
        ));
        let mut text = String::<96>::new();
        let _ = disassemble(&machine.cpu.memory, r.program_counter, &mut text);
        self.print(format_args!("{text}\r\n"));
    }

    fn memory<'a>(&mut self, machine: &Machine, words: &mut impl Iterator<Item = &'a str>) {
        let Some(address) = words.next().and_then(parse_hex_u16) else {
            self.print(format_args!("ERR usage: m ADDR [LEN]\r\n"));
            return;
        };
        let length = words.next().and_then(parse_hex_u16).map_or(64, usize::from);
        let length = length.min(4096);
        for offset in (0..length).step_by(16) {
            let base = usize::from(address) + offset;
            if base > usize::from(u16::MAX) {
                break;
            }
            let count = 16.min(length - offset).min(65_536 - base);
            self.print(format_args!("{base:04X}:"));
            for index in 0..16 {
                if index < count {
                    let address =
                        u16::try_from(base + index).expect("memory dump address is bounded");
                    self.print(format_args!(" {:02X}", machine.cpu.memory.peek(address)));
                } else {
                    self.print(format_args!("   "));
                }
            }
            self.print(format_args!("  "));
            for index in 0..count {
                let address = u16::try_from(base + index).expect("memory dump address is bounded");
                let byte = machine.cpu.memory.peek(address);
                let printable = if (0x20..0x7F).contains(&byte) {
                    byte
                } else {
                    b'.'
                };
                let _ = self.output.push_back(printable);
            }
            self.print(format_args!("\r\n"));
        }
    }

    fn write_memory<'a>(
        &mut self,
        machine: &mut Machine,
        words: &mut impl Iterator<Item = &'a str>,
    ) {
        let Some(mut address) = words.next().and_then(parse_hex_u16) else {
            self.print(format_args!("ERR usage: w ADDR BB [BB..]\r\n"));
            return;
        };
        let mut count = 0usize;
        for word in words {
            let Some(value) = parse_hex_u8(word) else {
                self.print(format_args!("ERR invalid byte {word}\r\n"));
                return;
            };
            machine.cpu.memory.poke(address, value);
            address = address.wrapping_add(1);
            count += 1;
        }
        self.print(format_args!("OK wrote {count} byte(s)\r\n"));
    }

    fn go<'a>(&mut self, machine: &mut Machine, words: &mut impl Iterator<Item = &'a str>) {
        if let Some(word) = words.next() {
            let Some(address) = parse_hex_u16(word) else {
                self.print(format_args!("ERR address out of range\r\n"));
                return;
            };
            self.start_at(machine, address);
            return;
        }
        machine.reset();
        self.run_cycles = 0;
        self.running = true;
        self.skip_breakpoint = false;
        self.print(format_args!(
            "OK running from ${:04X} (Ctrl-C stops)\r\n",
            machine.cpu.registers.program_counter
        ));
    }

    fn single_step<'a>(
        &mut self,
        machine: &mut Machine,
        words: &mut impl Iterator<Item = &'a str>,
    ) {
        let count = words.next().and_then(parse_hex_u16).unwrap_or(1);
        for _ in 0..count {
            let _ = machine.step();
            self.registers(machine);
        }
    }

    fn unassemble<'a>(&mut self, machine: &Machine, words: &mut impl Iterator<Item = &'a str>) {
        let mut address = words
            .next()
            .and_then(parse_hex_u16)
            .unwrap_or(machine.cpu.registers.program_counter);
        let count = words.next().and_then(parse_hex_u16).unwrap_or(0x10);
        for _ in 0..count {
            let mut text = String::<96>::new();
            let len = disassemble(&machine.cpu.memory, address, &mut text).unwrap_or(1);
            self.print(format_args!("{address:04X}  {text}\r\n"));
            address = address.wrapping_add(len);
        }
    }

    fn breakpoint<'a>(&mut self, words: &mut impl Iterator<Item = &'a str>) {
        match words.next() {
            Some("-") => {
                self.breakpoint = None;
                self.print(format_args!("OK breakpoint cleared\r\n"));
            }
            Some(word) => match parse_hex_u16(word) {
                Some(address) => {
                    self.breakpoint = Some(address);
                    self.print(format_args!("OK breakpoint at ${address:04X}\r\n"));
                }
                None => self.print(format_args!("ERR invalid breakpoint\r\n")),
            },
            None => match self.breakpoint {
                Some(address) => self.print(format_args!("Breakpoint at ${address:04X}\r\n")),
                None => self.print(format_args!("No breakpoint set\r\n")),
            },
        }
    }

    fn speed<'a>(&mut self, words: &mut impl Iterator<Item = &'a str>) {
        if let Some(value) = words.next().and_then(|word| word.parse::<u32>().ok()) {
            self.speed_khz = value;
        }
        if self.speed_khz == 0 {
            self.print(format_args!("No speed limit\r\n"));
        } else {
            let speed = self.speed_khz;
            self.print(format_args!("Speed limit {speed} kHz\r\n"));
        }
    }

    fn display(&mut self, machine: &Machine) {
        self.print(format_args!("+----------------+\r\n"));
        for row in machine.cpu.memory.lcd() {
            let _ = self.output.push_back(b'|');
            for byte in row {
                let _ = self.output.push_back(if (0x20..0x7F).contains(byte) {
                    *byte
                } else {
                    b'.'
                });
            }
            self.print(format_args!("|\r\n"));
        }
        let (row, col) = machine.cpu.memory.lcd_cursor();
        let light = if machine.cpu.memory.lcd_backlight() {
            "on"
        } else {
            "off"
        };
        self.print(format_args!(
            "+----------------+\r\nCursor row {row} col {col}, backlight {light}"
        ));
        match machine.cpu.memory.display_address() {
            Some(address) => self.print(format_args!(", display at I2C address {address:02X}\r\n")),
            None => self.print(format_args!(", display NOT DETECTED\r\n")),
        }
    }

    fn load<'a>(&mut self, words: &mut impl Iterator<Item = &'a str>) {
        let values = (
            words.next().and_then(parse_hex_u16),
            words.next().and_then(parse_hex_u32),
            words.next().and_then(parse_hex_u16),
        );
        let (Some(start), Some(length), Some(expected_crc)) = values else {
            self.print(format_args!("ERR usage: l ADDR LEN CRC\r\n"));
            return;
        };
        if length == 0 || u32::from(start) + length > 65_536 {
            self.print(format_args!("ERR load must lie within $0000-$FFFF\r\n"));
            return;
        }
        let end = u32::from(start) + length;
        if u32::from(start) < u32::from(IO_PAGE) + 0x100 && end > u32::from(IO_PAGE) {
            self.print(format_args!(
                "ERR load overlaps the I/O page $F000-$F0FF\r\n"
            ));
            return;
        }
        self.loading = Some(Loader {
            start,
            cursor: start,
            remaining: length,
            expected_crc,
            crc: 0xFFFF,
        });
        self.print(format_args!("READY\r\n"));
    }

    fn feed_loader(&mut self, machine: &mut Machine, byte: u8) -> bool {
        let Some(mut loader) = self.loading.take() else {
            return false;
        };
        machine.cpu.memory.poke(loader.cursor, byte);
        loader.cursor = loader.cursor.wrapping_add(1);
        loader.remaining -= 1;
        loader.crc = crc16_update(loader.crc, byte);
        if loader.remaining == 0 {
            let length = u32::from(loader.cursor.wrapping_sub(loader.start));
            if loader.crc == loader.expected_crc {
                self.print(format_args!(
                    "OK loaded {length} bytes at ${:04X}-${:04X}\r\n> ",
                    loader.start,
                    loader.cursor.wrapping_sub(1)
                ));
            } else {
                self.print(format_args!(
                    "ERR checksum {:04X}, expected {:04X}\r\n> ",
                    loader.crc, loader.expected_crc
                ));
            }
        } else {
            self.loading = Some(loader);
        }
        true
    }

    fn stop(&mut self, machine: &Machine, reason: StopReason) {
        self.running = false;
        let message = match reason {
            StopReason::User => "Stopped",
            StopReason::Breakpoint => "Breakpoint",
            StopReason::UnhandledBrk => "BRK (no IRQ/BRK vector set)",
            StopReason::EndlessLoop => "Halted in endless loop",
            StopReason::CpuStopped => "CPU stopped",
        };
        let cycles = self.run_cycles;
        self.print(format_args!(
            "\r\n{message} at ${:04X} after {cycles} cycles\r\n",
            machine.cpu.registers.program_counter
        ));
        self.registers(machine);
        self.print(format_args!("> "));
    }

    fn print(&mut self, args: fmt::Arguments<'_>) {
        struct Output<'a>(&'a mut Deque<u8, 8192>);
        impl Write for Output<'_> {
            fn write_str(&mut self, text: &str) -> fmt::Result {
                for byte in text.bytes() {
                    self.0.push_back(byte).map_err(|_| fmt::Error)?;
                }
                Ok(())
            }
        }
        let _ = Output(&mut self.output).write_fmt(args);
    }
}

fn parse_hex_u16(word: &str) -> Option<u16> {
    u16::from_str_radix(word.strip_prefix('$').unwrap_or(word), 16).ok()
}

fn parse_hex_u32(word: &str) -> Option<u32> {
    u32::from_str_radix(word.strip_prefix('$').unwrap_or(word), 16).ok()
}

fn parse_hex_u8(word: &str) -> Option<u8> {
    u8::from_str_radix(word.strip_prefix('$').unwrap_or(word), 16).ok()
}

fn crc16_update(mut crc: u16, byte: u8) -> u16 {
    crc ^= u16::from(byte) << 8;
    for _ in 0..8 {
        crc = if crc & 0x8000 == 0 {
            crc << 1
        } else {
            (crc << 1) ^ 0x1021
        };
    }
    crc
}

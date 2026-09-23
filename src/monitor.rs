//! Serial monitor: loads programs, runs the CPU in batches between checks for
//! input, and provides debugging commands.  Type `h` in a terminal for help.
//!
//! Output uses `\n` line endings, which go to the terminal as `\r\n`, as
//! the Pico SDK's USB serial does.  That includes bytes the 6502 sends.

use core::fmt::{self, Write as _};

use heapless::{String, Vec};

use crate::bus::{IO_PAGE, VEC_IRQ};
use crate::disasm::disasm;
use crate::lcd::{LCD_COLS, LcdHardware};
use crate::machine::Machine;
use crate::platform::Platform;

const LINE_LEN: usize = 80;
const CTRL_C: u8 = 0x03;
/// Give up on an upload after this long without a byte.
const LOAD_TIMEOUT_US: u32 = 2_000_000;
const IDLE_POLL_US: u32 = 10_000;
/// Emulated time per batch when speed-limited.
const BATCH_US: u64 = 10_000;
/// Instructions per batch when not speed-limited.
const UNLIMITED_STEPS: u32 = 20_000;

const HELP: &str = "Numbers are hex unless noted.
  r                 show CPU registers
  m ADDR [LEN]      dump memory
  w ADDR BB [BB..]  write bytes (I/O registers work too: w F001 41)
  g [ADDR]          reset CPU and run from ADDR (becomes the reset vector),
                    or from the current reset vector
  c                 continue from the current PC
  s [N]             single-step N instructions
  u [ADDR [N]]      disassemble N instructions (default 10); u alone continues
  x                 reset CPU without running
  b [ADDR | -]      show, set or clear the breakpoint
  t [KHZ]           show or set the speed limit, decimal kHz (0 = unlimited)
  d                 show what is on the LCD
  i                 check the display's I2C bus and look for it again
  l ADDR LEN CRC    receive LEN raw bytes (used by tools/upload.py)
  Ctrl-C            stop the running program
";

/// Why a running program stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// Ctrl-C.
    User,
    /// The program counter reached the breakpoint.
    Breakpoint,
    /// `BRK` while the IRQ/BRK vector is `$0000`.
    UnhandledBrk,
    /// An instruction jumped or branched to itself.
    EndlessLoop,
}

impl StopReason {
    const fn message(self) -> &'static str {
        match self {
            Self::User => "Stopped",
            Self::Breakpoint => "Breakpoint",
            Self::UnhandledBrk => "BRK (no IRQ/BRK vector set)",
            Self::EndlessLoop => "Halted in endless loop",
        }
    }
}

/// What is left of a batch of instructions.
struct Slice {
    steps: u32,
    budget: u64,
    spent: u64,
}

/// Why [`Monitor::run_slice`] returned.
struct SliceEnd {
    why: Option<StopReason>,
    /// The 6502 wrote to `SERIAL_DATA`; pass the bytes on and carry on.
    output: bool,
}

impl SliceEnd {
    const fn stop(why: StopReason) -> Self {
        Self {
            why: Some(why),
            output: false,
        }
    }
}

/// The monitor's state.
#[allow(clippy::struct_excessive_bools)]
pub struct Monitor {
    line: Vec<u8, LINE_LEN>,
    last_was_cr: bool,
    running: bool,
    /// A stock 6502 runs at 1 MHz; 0 means as fast as possible.
    speed_khz: u32,
    breakpoint: Option<u16>,
    /// Lets `c` continue from the breakpoint it stopped at.
    skip_breakpoint: bool,
    throttle_start_us: u64,
    throttle_cycles: u64,
    run_start_us: u64,
    run_cycles: u64,
    /// Where a bare `u` continues; `None` means at PC.
    unassemble_next: Option<u16>,
    last_stop: Option<StopReason>,
    /// Where the current or last run started.
    run_start_pc: u16,
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Writes formatted text to the terminal.
macro_rules! out {
    ($io:expr, $($arg:tt)*) => {
        print($io, format_args!($($arg)*)).await
    };
}

impl Monitor {
    /// An idle monitor with the 1 MHz speed limit.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            line: Vec::new(),
            last_was_cr: false,
            running: false,
            speed_khz: 1000,
            breakpoint: None,
            skip_breakpoint: false,
            throttle_start_us: 0,
            throttle_cycles: 0,
            run_start_us: 0,
            run_cycles: 0,
            unassemble_next: None,
            last_stop: None,
            run_start_pc: 0,
        }
    }

    /// Whether a program is running.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.running
    }

    /// Why the last run stopped, once one has.
    #[must_use]
    pub const fn last_stop(&self) -> Option<StopReason> {
        self.last_stop
    }

    /// Where the current or last run started.
    #[must_use]
    pub const fn run_start_pc(&self) -> u16 {
        self.run_start_pc
    }

    /// The speed limit in kHz; 0 means unlimited.
    #[must_use]
    pub const fn speed_khz(&self) -> u32 {
        self.speed_khz
    }

    /// Prints the banner.
    pub async fn init<H: LcdHardware, P: Platform>(&mut self, m: &Machine<H>, io: &mut P) {
        let display = if m.bus.lcd().present() {
            ""
        } else {
            " (display not detected)"
        };
        out!(
            io,
            "\nPico 6502: Fake6502 CPU, 64K RAM, 16x2 LCD{display}\nType h for help.\n"
        );
    }

    /// Sets the reset vector to `address`, resets the CPU and runs.
    pub async fn go<H: LcdHardware, P: Platform>(
        &mut self,
        m: &mut Machine<H>,
        io: &mut P,
        address: u16,
    ) {
        m.set_reset_vector(address);
        self.run_from_reset(m, io).await;
    }

    /// Handles input, then runs a batch of the program if one is running.
    /// Call repeatedly from the main loop.
    pub async fn poll<H: LcdHardware, P: Platform>(&mut self, m: &mut Machine<H>, io: &mut P) {
        let mut c = io.getc(if self.running { 0 } else { IDLE_POLL_US }).await;
        while let Some(byte) = c {
            // Swallow the LF of a CR LF pair so it neither repeats a command
            // nor reaches a program that was just started.
            let lf_after_cr = byte == b'\n' && self.last_was_cr;
            self.last_was_cr = byte == b'\r';
            if lf_after_cr {
                // nothing
            } else if !self.running {
                self.handle_key(m, io, byte).await;
            } else if byte == CTRL_C {
                self.stop(m, io, StopReason::User).await;
            } else {
                m.bus.push_serial_rx(byte);
            }
            c = io.getc(0).await;
        }
        if self.running {
            self.run_batch(m, io).await;
        }
    }

    // ------------------ CPU state ------------------------------------------

    async fn show_registers<H: LcdHardware, P: Platform>(&mut self, m: &Machine<H>, io: &mut P) {
        const NAMES: &[u8; 8] = b"NV-BDIZC";
        let cpu = &m.cpu;
        let p = cpu.status();
        let mut flags = [0u8; 8];
        for (i, flag) in flags.iter_mut().enumerate() {
            *flag = if p & (0x80 >> i) == 0 { b'.' } else { NAMES[i] };
        }
        let flags = core::str::from_utf8(&flags).unwrap_or("????????");
        let mut text = String::<48>::new();
        let _ = disassemble(m, cpu.pc, &mut text);
        out!(
            io,
            "PC={:04X} A={:02X} X={:02X} Y={:02X} SP={:02X} P={flags}  {text}\n",
            cpu.pc,
            cpu.a,
            cpu.x,
            cpu.y,
            cpu.sp
        );
        self.unassemble_next = None;
    }

    fn begin_run<P: Platform>(&mut self, io: &mut P, pc: u16) {
        self.running = true;
        self.run_start_pc = pc;
        self.run_start_us = io.time_us();
        self.throttle_start_us = self.run_start_us;
        self.throttle_cycles = 0;
        self.run_cycles = 0;
        io.led(true);
    }

    async fn stop<H: LcdHardware, P: Platform>(
        &mut self,
        m: &Machine<H>,
        io: &mut P,
        why: StopReason,
    ) {
        self.running = false;
        self.last_stop = Some(why);
        io.led(false);
        let us = io.time_us().saturating_sub(self.run_start_us);
        out!(
            io,
            "\n{} at ${:04X} after {} cycles",
            why.message(),
            m.cpu.pc,
            self.run_cycles
        );
        if us >= 100_000 {
            out!(io, " ({} kHz)", self.run_cycles * 1000 / us);
        }
        out!(io, "\n");
        self.show_registers(m, io).await;
        prompt(io).await;
    }

    async fn run_from_reset<H: LcdHardware, P: Platform>(
        &mut self,
        m: &mut Machine<H>,
        io: &mut P,
    ) {
        m.reset();
        m.bus.clear_serial_rx();
        self.skip_breakpoint = false;
        out!(io, "OK running from ${:04X} (Ctrl-C stops)\n", m.cpu.pc);
        self.begin_run(io, m.cpu.pc);
    }

    /// Runs one slice of the program, then sleeps if it is ahead of the speed
    /// limit.
    async fn run_batch<H: LcdHardware, P: Platform>(&mut self, m: &mut Machine<H>, io: &mut P) {
        let limited = self.speed_khz != 0;
        let budget = if limited {
            u64::from(self.speed_khz) * BATCH_US / 1000
        } else {
            u64::MAX
        };
        let steps = if limited { u32::MAX } else { UNLIMITED_STEPS };
        let mut slice = Slice {
            steps,
            budget,
            spent: 0,
        };
        let why = loop {
            let end = self.run_slice(m, &mut slice);
            if end.output {
                send_serial_output(m, io).await;
            }
            if end.why.is_some() || !end.output {
                break end.why;
            }
        };
        let spent = slice.spent;
        self.run_cycles += spent;
        io.flush().await;
        if let Some(why) = why {
            self.stop(m, io, why).await;
            return;
        }

        if limited {
            self.throttle_cycles += spent;
            let due =
                self.throttle_start_us + self.throttle_cycles * 1000 / u64::from(self.speed_khz);
            let now = io.time_us();
            if due > now {
                io.sleep_us(due - now).await;
            } else if now - due > 100_000 {
                // Fell behind (slow LCD writes); start over rather than race
                // to catch up.
                self.throttle_start_us = now;
                self.throttle_cycles = 0;
            }
        }
    }

    /// Runs instructions until the slice's step or cycle budget is spent, the
    /// program stops, or it has written to `SERIAL_DATA`.  This is the
    /// emulator's inner loop, so the firmware runs it from RAM.
    #[inline(never)]
    #[cfg_attr(
        feature = "firmware",
        allow(unsafe_code),
        unsafe(link_section = ".data.ram_func")
    )]
    fn run_slice<H: LcdHardware>(&mut self, m: &mut Machine<H>, slice: &mut Slice) -> SliceEnd {
        while slice.steps > 0 && slice.spent < slice.budget {
            let pc = m.cpu.pc;
            if self.breakpoint == Some(pc) && !self.skip_breakpoint {
                return SliceEnd::stop(StopReason::Breakpoint);
            }
            self.skip_breakpoint = false;
            if m.bus.peek(pc) == 0x00 && m.word(VEC_IRQ) == 0x0000 {
                return SliceEnd::stop(StopReason::UnhandledBrk);
            }
            slice.spent += u64::from(m.step());
            slice.steps -= 1;
            // For example "done: jmp done": nothing can ever change.
            let stuck = m.cpu.pc == pc;
            let end = SliceEnd {
                why: stuck.then_some(StopReason::EndlessLoop),
                output: m.bus.has_serial_tx(),
            };
            if end.why.is_some() || end.output {
                return end;
            }
        }
        SliceEnd {
            why: None,
            output: false,
        }
    }

    // ------------------ Commands -------------------------------------------

    async fn execute<H: LcdHardware, P: Platform>(
        &mut self,
        m: &mut Machine<H>,
        io: &mut P,
        line: &[u8],
    ) {
        let mut args = Args(line);
        args.skip_spaces();
        let Some((&command, rest)) = args.0.split_first() else {
            return;
        };
        let mut args = Args(rest);
        match command.to_ascii_lowercase() {
            b'h' | b'?' => put(io, HELP.as_bytes()).await,
            b'r' => self.show_registers(m, io).await,
            b'm' => cmd_memory(m, io, &mut args).await,
            b'w' => cmd_write(m, io, &mut args).await,
            b'g' => self.cmd_go(m, io, &mut args).await,
            b'c' => {
                out!(io, "OK continuing at ${:04X} (Ctrl-C stops)\n", m.cpu.pc);
                self.skip_breakpoint = true;
                self.begin_run(io, m.cpu.pc);
            }
            b's' => self.cmd_step(m, io, &mut args).await,
            b'u' => self.cmd_unassemble(m, io, &mut args).await,
            b'x' => {
                m.reset();
                self.show_registers(m, io).await;
            }
            b'b' => self.cmd_breakpoint(io, &mut args).await,
            b't' => self.cmd_speed(io, &mut args).await,
            b'd' => cmd_display(m, io).await,
            b'i' => cmd_i2c(m, io).await,
            b'l' => cmd_load(m, io, &mut args).await,
            _ => out!(io, "ERR unknown command, h for help\n"),
        }
    }

    async fn cmd_go<H: LcdHardware, P: Platform>(
        &mut self,
        m: &mut Machine<H>,
        io: &mut P,
        args: &mut Args<'_>,
    ) {
        match args.number(16) {
            None => self.run_from_reset(m, io).await,
            Some(address) => match u16::try_from(address) {
                Ok(address) => self.go(m, io, address).await,
                Err(_) => out!(io, "ERR address out of range\n"),
            },
        }
    }

    async fn cmd_step<H: LcdHardware, P: Platform>(
        &mut self,
        m: &mut Machine<H>,
        io: &mut P,
        args: &mut Args<'_>,
    ) {
        let n = args.number(16).unwrap_or(1);
        for _ in 0..n {
            m.step();
            send_serial_output(m, io).await;
            self.show_registers(m, io).await;
            if io.getc(0).await == Some(CTRL_C) {
                break;
            }
        }
    }

    async fn cmd_unassemble<H: LcdHardware, P: Platform>(
        &mut self,
        m: &Machine<H>,
        io: &mut P,
        args: &mut Args<'_>,
    ) {
        let (mut address, count) = match args.number(16) {
            Some(address) => {
                let Ok(address) = u16::try_from(address) else {
                    out!(io, "ERR usage: u [ADDR [N]]\n");
                    return;
                };
                (address, args.number(16).unwrap_or(0x10))
            }
            None => (self.unassemble_next.unwrap_or(m.cpu.pc), 0x10),
        };
        for _ in 0..count {
            let mut text = String::<48>::new();
            let len = disassemble(m, address, &mut text).unwrap_or(1);
            out!(io, "{address:04X}  {text}\n");
            address = address.wrapping_add(len);
        }
        self.unassemble_next = Some(address);
    }

    async fn cmd_breakpoint<P: Platform>(&mut self, io: &mut P, args: &mut Args<'_>) {
        args.skip_spaces();
        if args.0.first() == Some(&b'-') {
            self.breakpoint = None;
            out!(io, "OK breakpoint cleared\n");
        } else if let Some(address) = args.number(16).and_then(|a| u16::try_from(a).ok()) {
            self.breakpoint = Some(address);
            out!(io, "OK breakpoint at ${address:04X}\n");
        } else if let Some(address) = self.breakpoint {
            out!(io, "Breakpoint at ${address:04X}\n");
        } else {
            out!(io, "No breakpoint set\n");
        }
    }

    async fn cmd_speed<P: Platform>(&mut self, io: &mut P, args: &mut Args<'_>) {
        if let Some(khz) = args.number(10) {
            self.speed_khz = khz;
        }
        if self.speed_khz == 0 {
            out!(io, "No speed limit\n");
        } else {
            out!(io, "Speed limit {} kHz\n", self.speed_khz);
        }
    }

    // ------------------ Input ----------------------------------------------

    async fn handle_key<H: LcdHardware, P: Platform>(
        &mut self,
        m: &mut Machine<H>,
        io: &mut P,
        c: u8,
    ) {
        match c {
            b'\r' | b'\n' => {
                out!(io, "\n");
                let line = core::mem::take(&mut self.line);
                self.execute(m, io, &line).await;
                if !self.running {
                    prompt(io).await;
                }
            }
            CTRL_C => {
                self.line.clear();
                out!(io, "^C\n");
                prompt(io).await;
            }
            0x08 | 0x7F => {
                if self.line.pop().is_some() {
                    io.write(b"\x08 \x08").await;
                    io.flush().await;
                }
            }
            0x20..0x7F if !self.line.is_full() => {
                let _ = self.line.push(c);
                io.write(&[c]).await;
                io.flush().await;
            }
            _ => {}
        }
    }
}

async fn cmd_memory<H: LcdHardware, P: Platform>(m: &Machine<H>, io: &mut P, args: &mut Args<'_>) {
    let Some(address) = args.number(16).filter(|&a| a <= 0xFFFF) else {
        out!(io, "ERR usage: m ADDR [LEN]\n");
        return;
    };
    let len = args.number(16).unwrap_or(64);
    let mut i = 0u32;
    while i < len && address + i <= 0xFFFF {
        let base = address + i;
        let n = 16.min(len - i).min(0x1_0000 - base);
        let mut text = String::<80>::new();
        let _ = write!(text, "{base:04X}:");
        for j in 0..16 {
            if j < n {
                let _ = write!(text, " {:02X}", peek(m, base + j));
            } else {
                let _ = text.push_str("   ");
            }
        }
        let _ = text.push_str("  ");
        for j in 0..n {
            let _ = text.push(printable(peek(m, base + j)));
        }
        let _ = text.push('\n');
        put(io, text.as_bytes()).await;
        i += 16;
    }
}

async fn cmd_write<H: LcdHardware, P: Platform>(
    m: &mut Machine<H>,
    io: &mut P,
    args: &mut Args<'_>,
) {
    let Some(address) = args.number(16).and_then(|a| u16::try_from(a).ok()) else {
        out!(io, "ERR usage: w ADDR BB [BB..]\n");
        return;
    };
    let mut count = 0u16;
    while let Some(value) = args.number(16) {
        let Ok(value) = u8::try_from(value) else {
            out!(io, "ERR byte value {value:X} too large\n");
            return;
        };
        m.bus.poke(address.wrapping_add(count), value);
        count = count.wrapping_add(1);
    }
    let plural = if count == 1 { "" } else { "s" };
    out!(io, "OK wrote {count} byte{plural}\n");
}

async fn cmd_i2c<H: LcdHardware, P: Platform>(m: &mut Machine<H>, io: &mut P) {
    let mut text = String::<1024>::new();
    m.bus.lcd_mut().diagnose(&mut text);
    put(io, text.as_bytes()).await;
    m.bus.lcd_mut().init();
    match m.bus.lcd().address().filter(|_| m.bus.lcd().present()) {
        Some(address) => out!(io, "OK display found at I2C address {address:02X}\n"),
        None => out!(io, "ERR display not found\n"),
    }
}

async fn cmd_display<H: LcdHardware, P: Platform>(m: &Machine<H>, io: &mut P) {
    let lcd = m.bus.lcd();
    out!(io, "+----------------+\n");
    for row in lcd.text() {
        let mut text = String::<{ LCD_COLS + 4 }>::new();
        let _ = text.push('|');
        for &c in row {
            let _ = text.push(printable(c));
        }
        let _ = text.push_str("|\n");
        put(io, text.as_bytes()).await;
    }
    out!(io, "+----------------+\n");
    let light = if lcd.backlight_on() { "on" } else { "off" };
    out!(
        io,
        "Cursor row {} col {}, backlight {light}, ",
        lcd.row(),
        lcd.col()
    );
    match lcd.address().filter(|_| lcd.present()) {
        Some(address) => out!(io, "display at I2C address {address:02X}\n"),
        None => out!(io, "display NOT DETECTED\n"),
    }
}

/// `l ADDR LEN CRC`: reply `READY`, receive LEN raw bytes into memory, then
/// reply `OK` or `ERR`.  A failed load can leave partial data behind.
async fn cmd_load<H: LcdHardware, P: Platform>(
    m: &mut Machine<H>,
    io: &mut P,
    args: &mut Args<'_>,
) {
    let (Some(address), Some(len), Some(crc)) = (args.number(16), args.number(16), args.number(16))
    else {
        out!(io, "ERR usage: l ADDR LEN CRC\n");
        return;
    };
    if address > 0xFFFF || len == 0 || len > 0x1_0000 - address {
        out!(io, "ERR load must lie within $0000-$FFFF\n");
        return;
    }
    let io_page = u32::from(IO_PAGE);
    if address < io_page + 0x100 && address + len > io_page {
        out!(io, "ERR load overlaps the I/O page $F000-$F0FF\n");
        return;
    }
    out!(io, "READY\n");
    io.flush().await;

    let mut sum = 0xFFFF;
    for i in 0..len {
        let Some(c) = io.getc(LOAD_TIMEOUT_US).await else {
            out!(io, "ERR timeout after {i} of {len} bytes\n");
            return;
        };
        m.bus.ram_mut()[(address + i) as usize] = c;
        sum = crc16_update(sum, c);
    }
    if u32::from(sum) == crc {
        out!(
            io,
            "OK loaded {len} bytes at ${address:04X}-${:04X}\n",
            address + len - 1
        );
    } else {
        out!(io, "ERR checksum {sum:04X}, expected {crc:04X}\n");
    }
}

/// CRC-16/CCITT-FALSE, the same as Python's `binascii.crc_hqx(data, 0xFFFF)`.
const fn crc16_update(mut crc: u16, byte: u8) -> u16 {
    crc ^= (byte as u16) << 8;
    let mut i = 0;
    while i < 8 {
        crc = if crc & 0x8000 == 0 {
            crc << 1
        } else {
            (crc << 1) ^ 0x1021
        };
        i += 1;
    }
    crc
}

// ------------------ Helpers -------------------------------------------------

/// The rest of a command line, parsed a number at a time.
struct Args<'a>(&'a [u8]);

impl Args<'_> {
    fn skip_spaces(&mut self) {
        while let Some((&b' ', rest)) = self.0.split_first() {
            self.0 = rest;
        }
    }

    /// The next number, with an optional leading `$`.  It must start with a
    /// hex digit, even in base 10, and saturates at `u32::MAX`.  A base-16
    /// number may have a `0x` prefix.
    fn number(&mut self, base: u32) -> Option<u32> {
        self.skip_spaces();
        let mut s = self.0;
        if let Some((&b'$', rest)) = s.split_first() {
            s = rest;
        }
        if !s.first().is_some_and(u8::is_ascii_hexdigit) {
            return None;
        }
        if base == 16
            && s.len() > 2
            && s[0] == b'0'
            && s[1].eq_ignore_ascii_case(&b'x')
            && s[2].is_ascii_hexdigit()
        {
            s = &s[2..];
        }
        let digits = s
            .iter()
            .take_while(|c| char::from(**c).is_digit(base))
            .count();
        if digits == 0 {
            return None;
        }
        let value = s[..digits].iter().fold(0u32, |value, &c| {
            let digit = char::from(c).to_digit(base).unwrap_or(0);
            value.saturating_mul(base).saturating_add(digit)
        });
        self.0 = &s[digits..];
        Some(value)
    }
}

fn peek<H: LcdHardware>(m: &Machine<H>, address: u32) -> u8 {
    m.bus.peek(u16::try_from(address).unwrap_or(0))
}

const fn printable(c: u8) -> char {
    if c >= 0x20 && c < 0x7F {
        c as char
    } else {
        '.'
    }
}

fn disassemble<H: LcdHardware>(
    m: &Machine<H>,
    address: u16,
    text: &mut impl fmt::Write,
) -> Result<u16, fmt::Error> {
    let bytes = [
        m.bus.peek(address),
        m.bus.peek(address.wrapping_add(1)),
        m.bus.peek(address.wrapping_add(2)),
    ];
    disasm(address, bytes, text)
}

/// Passes on the bytes the 6502 has written to `SERIAL_DATA`.
async fn send_serial_output<H: LcdHardware, P: Platform>(m: &mut Machine<H>, io: &mut P) {
    while let Some(byte) = m.bus.pop_serial_tx() {
        put(io, &[byte]).await;
    }
}

async fn prompt<P: Platform>(io: &mut P) {
    io.write(b"> ").await;
    io.flush().await;
}

/// Writes text, turning each `\n` into `\r\n`.
async fn put<P: Platform>(io: &mut P, mut text: &[u8]) {
    while let Some(end) = text.iter().position(|&c| c == b'\n') {
        io.write(&text[..end]).await;
        io.write(b"\r\n").await;
        text = &text[end + 1..];
    }
    if !text.is_empty() {
        io.write(text).await;
    }
}

async fn print<P: Platform>(io: &mut P, args: fmt::Arguments<'_>) {
    let mut text = String::<128>::new();
    let _ = text.write_fmt(args);
    put(io, text.as_bytes()).await;
}

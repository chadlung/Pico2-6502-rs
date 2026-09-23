//! Tests of the monitor, driven the way a terminal would drive it: a line at
//! a time, with a simulated clock.

use std::collections::VecDeque;

use embassy_futures::block_on;
use pico2_6502_rs::lcd::NoDisplay;
use pico2_6502_rs::machine::Machine;
use pico2_6502_rs::monitor::Monitor;
use pico2_6502_rs::platform::Platform;
use pico2_6502_rs::{DEMO_LOAD_ADDR, DEMO_PROGRAM};

/// A terminal whose clock only moves when the monitor waits.
#[derive(Default)]
struct Script {
    input: VecDeque<u8>,
    output: Vec<u8>,
    now_us: u64,
    led: bool,
}

// The script never waits, so these async functions don't await.
#[allow(clippy::unused_async_trait_impl)]
impl Platform for Script {
    async fn getc(&mut self, timeout_us: u32) -> Option<u8> {
        let byte = self.input.pop_front();
        if byte.is_none() {
            self.now_us += u64::from(timeout_us);
        }
        byte
    }

    async fn write(&mut self, bytes: &[u8]) {
        self.output.extend_from_slice(bytes);
    }

    async fn flush(&mut self) {}

    fn time_us(&mut self) -> u64 {
        self.now_us
    }

    async fn sleep_us(&mut self, us: u64) {
        self.now_us += us;
    }

    fn led(&mut self, on: bool) {
        self.led = on;
    }
}

struct Session {
    machine: Box<Machine<NoDisplay>>,
    monitor: Monitor,
    io: Script,
}

impl Session {
    /// Powers up with the demo, runs it, and discards the banner.
    fn new() -> Self {
        let mut machine = Box::new(Machine::new(NoDisplay));
        machine.init();
        machine
            .bus
            .load(DEMO_LOAD_ADDR, DEMO_PROGRAM)
            .expect("demo loads");
        let mut session = Self {
            machine,
            monitor: Monitor::new(),
            io: Script::default(),
        };
        block_on(async {
            session
                .monitor
                .init(&session.machine, &mut session.io)
                .await;
            session
                .monitor
                .go(&mut session.machine, &mut session.io, DEMO_LOAD_ADDR)
                .await;
        });
        session.settle();
        session.take_output();
        session
    }

    /// Polls until all input is read and no program runs.
    fn settle(&mut self) {
        for _ in 0..1_000_000 {
            if self.io.input.is_empty() && !self.monitor.is_running() {
                return;
            }
            block_on(self.monitor.poll(&mut self.machine, &mut self.io));
        }
        panic!("the monitor did not settle");
    }

    /// Polls `n` times, for example to let a program run.
    fn poll(&mut self, n: usize) {
        for _ in 0..n {
            block_on(self.monitor.poll(&mut self.machine, &mut self.io));
        }
    }

    /// Types `bytes` without waiting for the result.
    fn type_bytes(&mut self, bytes: &[u8]) {
        self.io.input.extend(bytes);
    }

    /// Types each line, waiting for the monitor after each one, and returns
    /// the output with CR LF turned into LF.
    fn send(&mut self, text: &str) -> String {
        for line in text.split_inclusive('\n') {
            self.type_bytes(line.as_bytes());
            self.settle();
        }
        self.take_output()
    }

    /// Types everything at once, as an uploader does, then waits for the
    /// monitor.
    fn send_all(&mut self, bytes: &[u8]) -> String {
        self.type_bytes(bytes);
        self.settle();
        self.take_output()
    }

    fn take_output(&mut self) -> String {
        let raw = String::from_utf8(std::mem::take(&mut self.io.output)).expect("ASCII");
        raw.replace("\r\n", "\n")
    }
}

#[test]
fn banner_and_demo() {
    let mut machine = Box::new(Machine::new(NoDisplay));
    machine.init();
    machine.bus.load(DEMO_LOAD_ADDR, DEMO_PROGRAM).unwrap();
    let mut monitor = Monitor::new();
    let mut io = Script::default();
    block_on(async {
        monitor.init(&machine, &mut io).await;
        monitor.go(&mut machine, &mut io, DEMO_LOAD_ADDR).await;
        while monitor.is_running() {
            monitor.poll(&mut machine, &mut io).await;
        }
    });
    let output = String::from_utf8(io.output).unwrap();
    assert_eq!(
        output,
        "\r\nPico 6502: Fake6502 CPU, 64K RAM, 16x2 LCD (display not detected)\r\n\
         Type h for help.\r\n\
         OK running from $0200 (Ctrl-C stops)\r\n\
         \r\nHalted in endless loop at $0222 after 589 cycles\r\n\
         PC=0222 A=00 X=FF Y=0F SP=FF P=..-..IZ.  4C 22 02  JMP $0222\r\n> "
    );
}

#[test]
fn readme_example_1_look_at_the_demo() {
    let mut s = Session::new();
    assert_eq!(
        s.send("d\n"),
        "d\n+----------------+\n|RASPBERRY PICO 2|\n|HELLO FROM 6502 |\n+----------------+\n\
         Cursor row 1 col 15, backlight on, display NOT DETECTED\n> "
    );
    assert_eq!(
        s.send("u 0200 9\n"),
        "u 0200 9\n\
         0200  78        SEI\n\
         0201  D8        CLD\n\
         0202  A2 FF     LDX #$FF\n\
         0204  9A        TXS\n\
         0205  A9 01     LDA #$01\n\
         0207  8D 00 F0  STA $F000  ; LCD_CONTROL\n\
         020A  A9 36     LDA #$36\n\
         020C  A0 02     LDY #$02\n\
         020E  20 25 02  JSR $0225\n> "
    );
    let more = s.send("u\n");
    assert!(more.starts_with("u\n0211  A9 01     LDA #$01\n"), "{more}");
    assert_eq!(more.lines().count(), 18, "16 instructions, echo and prompt");
}

#[test]
fn readme_example_2_type_in_a_program() {
    let mut s = Session::new();
    assert_eq!(
        s.send("w 0300 A9 01 8D 00 F0 A9 48 8D 01 F0 A9 49 8D 01 F0 4C 0F 03\n"),
        "w 0300 A9 01 8D 00 F0 A9 48 8D 01 F0 A9 49 8D 01 F0 4C 0F 03\nOK wrote 18 bytes\n> "
    );
    assert_eq!(
        s.send("g 300\n"),
        "g 300\nOK running from $0300 (Ctrl-C stops)\n\n\
         Halted in endless loop at $030F after 21 cycles\n\
         PC=030F A=49 X=00 Y=00 SP=FD P=..-..I..  4C 0F 03  JMP $030F\n> "
    );
    let display = s.send("d\n");
    assert!(
        display.contains("|HI              |\n|                |"),
        "{display}"
    );
}

#[test]
fn readme_example_3_add_then_single_step() {
    let mut s = Session::new();
    s.send("w 0010 25 17\nw 0400 18 A5 10 65 11 85 12 00\n");
    assert_eq!(
        s.send("g 400\n"),
        "g 400\nOK running from $0400 (Ctrl-C stops)\n\n\
         BRK (no IRQ/BRK vector set) at $0407 after 11 cycles\n\
         PC=0407 A=3C X=00 Y=00 SP=FD P=..-..I..  00        BRK\n> "
    );
    assert_eq!(
        s.send("m 0010 3\n"),
        "m 0010 3\n0010: 25 17 3C                                         %.<\n> "
    );
    assert_eq!(
        s.send("x\ns 4\n"),
        "x\nPC=0400 A=00 X=00 Y=00 SP=FD P=..-..I..  18        CLC\n> \
         s 4\n\
         PC=0401 A=00 X=00 Y=00 SP=FD P=..-..I..  A5 10     LDA $10\n\
         PC=0403 A=25 X=00 Y=00 SP=FD P=..-..I..  65 11     ADC $11\n\
         PC=0405 A=3C X=00 Y=00 SP=FD P=..-..I..  85 12     STA $12\n\
         PC=0407 A=3C X=00 Y=00 SP=FD P=..-..I..  00        BRK\n> "
    );
}

#[test]
fn readme_example_4_serial_output_and_breakpoint() {
    let mut s = Session::new();
    s.send("w 0500 A2 00 BD 0E 05 F0 06 8D 10 F0 E8 D0 F5 00 48 45 4C 4C 4F 0D 0A 00\n");
    // The program's CR LF reaches the terminal as CR CR LF, as on the Pico.
    assert_eq!(
        s.send("g 500\n"),
        "g 500\nOK running from $0500 (Ctrl-C stops)\nHELLO\r\n\n\
         BRK (no IRQ/BRK vector set) at $050D after 114 cycles\n\
         PC=050D A=00 X=07 Y=00 SP=FD P=..-..IZ.  00        BRK\n> "
    );
    assert_eq!(s.send("b 0507\n"), "b 0507\nOK breakpoint at $0507\n> ");
    assert_eq!(
        s.send("g 500\n"),
        "g 500\nOK running from $0500 (Ctrl-C stops)\n\n\
         Breakpoint at $0507 after 8 cycles\n\
         PC=0507 A=48 X=00 Y=00 SP=FD P=..-..I..  8D 10 F0  STA $F010  ; SERIAL_DATA\n> "
    );
    assert_eq!(
        s.send("c\n"),
        "c\nOK continuing at $0507 (Ctrl-C stops)\nH\n\
         Breakpoint at $0507 after 15 cycles\n\
         PC=0507 A=45 X=01 Y=00 SP=FD P=..-..I..  8D 10 F0  STA $F010  ; SERIAL_DATA\n> "
    );
    assert_eq!(s.send("b\n"), "b\nBreakpoint at $0507\n> ");
    assert_eq!(s.send("b -\n"), "b -\nOK breakpoint cleared\n> ");
    let rest = s.send("c\n");
    assert!(
        rest.starts_with("c\nOK continuing at $0507 (Ctrl-C stops)\nELLO\r\n\nBRK"),
        "{rest}"
    );
    assert!(rest.contains("after 91 cycles"), "{rest}");
}

#[test]
fn readme_example_5_drive_the_lcd_from_the_prompt() {
    let mut s = Session::new();
    s.send(
        "w F000 01\nw F001 36\nw F001 35\nw F002 01\nw F003 0C\nw F001 30\nw F001 32\nw F004 00\n",
    );
    let display = s.send("d\n");
    assert!(
        display.contains(
            "|65              |\n|            02  |\n+----------------+\n\
             Cursor row 1 col 14, backlight off"
        ),
        "{display}"
    );
}

#[test]
fn readme_example_6_keyboard_input() {
    let mut s = Session::new();
    s.send("w 0200 A2 FF 9A A9 01 8D 00 F0 AD 11 F0 10 FB AD 10 F0\n");
    s.send("w 0210 C9 0D D0 02 A9 0A 8D 01 F0 8D 10 F0 4C 08 02\n");
    s.type_bytes(b"g 200\r\n");
    s.poll(20);
    s.type_bytes(b"Hi\r\nthere");
    s.poll(20);
    s.type_bytes(b"\x03");
    s.poll(1);
    let output = s.take_output();
    assert!(
        output.contains("OK running from $0200 (Ctrl-C stops)\nHi\nthere\nStopped at $02"),
        "{output:?}"
    );
    let display = s.send("d\n");
    assert!(
        display.contains("|Hi              |\n|there           |"),
        "{display}"
    );
}

#[test]
fn speed_limit_holds_near_1_mhz() {
    let mut s = Session::new();
    s.send("t 1000\nw 0300 E8 4C 00 03\n");
    s.type_bytes(b"g 300\n");
    s.poll(300);
    s.type_bytes(b"\x03");
    s.poll(1);
    let output = s.take_output();
    let khz: u64 = output
        .split_once(" kHz)")
        .and_then(|(before, _)| before.rsplit_once('('))
        .and_then(|(_, khz)| khz.parse().ok())
        .unwrap_or_else(|| panic!("no speed in {output:?}"));
    assert!((990..=1010).contains(&khz), "{khz} kHz");
    assert!(!s.io.led, "the LED goes out when the program stops");
}

#[test]
fn speed_command() {
    let mut s = Session::new();
    assert_eq!(s.send("t\n"), "t\nSpeed limit 1000 kHz\n> ");
    assert_eq!(s.send("t 0\n"), "t 0\nNo speed limit\n> ");
    assert_eq!(s.send("t 2500\n"), "t 2500\nSpeed limit 2500 kHz\n> ");
}

#[test]
fn loader_checks_crc_address_and_io_page() {
    let mut s = Session::new();
    let ok = s.send_all(b"l 0300 3 F508\nABC");
    assert_eq!(
        ok,
        "l 0300 3 F508\nREADY\nOK loaded 3 bytes at $0300-$0302\n> "
    );
    assert_eq!(&s.machine.bus.ram()[0x0300..0x0303], b"ABC");

    let bad = s.send_all(b"l 0300 2 0000\n\x01\x02");
    assert!(bad.contains("ERR checksum"), "{bad}");
    assert!(
        s.send("l EFFF 2 0\n")
            .contains("ERR load overlaps the I/O page")
    );
    assert!(
        s.send("l FFFF 2 0\n")
            .contains("ERR load must lie within $0000-$FFFF")
    );
    assert!(s.send("l 0300\n").contains("ERR usage: l ADDR LEN CRC"));
}

#[test]
fn loader_times_out() {
    let mut s = Session::new();
    let output = s.send_all(b"l 0300 4 0\nAB");
    assert!(
        output.ends_with("ERR timeout after 2 of 4 bytes\n> "),
        "{output}"
    );
}

#[test]
fn command_parsing_matches_the_original() {
    let mut s = Session::new();
    assert_eq!(s.send("w 0300 1\n"), "w 0300 1\nOK wrote 1 byte\n> ");
    assert_eq!(
        s.send("w 0300 1FF\n"),
        "w 0300 1FF\nERR byte value 1FF too large\n> "
    );
    assert_eq!(s.send("w\n"), "w\nERR usage: w ADDR BB [BB..]\n> ");
    assert_eq!(
        s.send("W $0300 $A9 $41\n"),
        "W $0300 $A9 $41\nOK wrote 2 bytes\n> ",
        "upper case and $ prefixes"
    );
    assert!(
        s.send("m0300 2\n").contains("0300: A9 41"),
        "no space needed"
    );
    assert_eq!(s.send("q\n"), "q\nERR unknown command, h for help\n> ");
    assert_eq!(s.send("g 10000\n"), "g 10000\nERR address out of range\n> ");
    assert_eq!(s.send("m\n"), "m\nERR usage: m ADDR [LEN]\n> ");
    assert_eq!(s.send("\n"), "\n> ");
    assert!(
        s.send("h\n")
            .contains("  i                 check the display's I2C bus")
    );
}

#[test]
fn line_editing() {
    let mut s = Session::new();
    s.type_bytes(b"x\x7Fd\x08r\r\n");
    s.settle();
    let output = s.take_output();
    assert!(
        output.starts_with("x\x08 \x08d\x08 \x08r\nPC="),
        "{output:?}"
    );
    assert_eq!(s.send("abc\x03"), "abc^C\n> ");
}

#[test]
fn ctrl_c_stops_a_program_and_c_continues() {
    let mut s = Session::new();
    s.send("w 0300 E8 4C 00 03\n");
    s.type_bytes(b"g 300\n");
    s.poll(5);
    s.type_bytes(b"\x03");
    s.poll(1);
    let stopped = s.take_output();
    assert!(stopped.contains("\nStopped at $030"), "{stopped}");
    s.type_bytes(b"c\r");
    s.poll(3);
    let resumed = s.take_output();
    assert!(resumed.contains("OK continuing at $030"), "{resumed}");
    assert!(s.monitor.is_running());
}

#[test]
fn display_troubleshooting_without_a_bus() {
    let mut s = Session::new();
    assert_eq!(
        s.send("i\n"),
        "i\nThe simulator has no I2C bus.\nERR display not found\n> "
    );
    assert!(
        s.send("d\n").contains("|                |"),
        "i clears the screen"
    );
}

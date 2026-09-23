//! Tests of the CPU, bus, display and disassembler.

use pico2_6502_rs::bus::{IO_LCD_DATA, IO_SERIAL_DATA, IO_SERIAL_STATUS, VEC_RESET};
use pico2_6502_rs::cpu::Memory;
use pico2_6502_rs::disasm::disasm;
use pico2_6502_rs::lcd::NoDisplay;
use pico2_6502_rs::machine::Machine;
use pico2_6502_rs::{DEMO_LOAD_ADDR, DEMO_PROGRAM};

fn machine_with(address: u16, program: &[u8]) -> Box<Machine<NoDisplay>> {
    let mut machine = Box::new(Machine::new(NoDisplay));
    machine.init();
    machine.bus.load(address, program).expect("program loads");
    machine.set_reset_vector(address);
    machine.reset();
    machine
}

fn run_until_stuck(machine: &mut Machine<NoDisplay>, limit: usize) {
    for _ in 0..limit {
        let pc = machine.cpu.pc;
        machine.step();
        if machine.cpu.pc == pc {
            return;
        }
    }
    panic!("program did not finish");
}

#[test]
fn built_in_demo_writes_both_lcd_rows() {
    let mut machine = machine_with(DEMO_LOAD_ADDR, DEMO_PROGRAM);
    run_until_stuck(&mut machine, 1_000);
    assert_eq!(machine.cpu.pc, 0x0222);
    assert_eq!(&machine.bus.lcd().text()[0], b"RASPBERRY PICO 2");
    assert_eq!(&machine.bus.lcd().text()[1], b"HELLO FROM 6502 ");
}

#[test]
fn reset_matches_fake6502() {
    let mut machine = machine_with(0x0300, &[0xEA]);
    machine.cpu.a = 0x12;
    machine.cpu.x = 0x34;
    machine.cpu.y = 0x56;
    machine.cpu.set_status(0xFF);
    machine.reset();
    assert_eq!(
        (machine.cpu.pc, machine.cpu.a, machine.cpu.x, machine.cpu.y),
        (0x0300, 0, 0, 0)
    );
    assert_eq!(machine.cpu.sp, 0xFD);
    assert_eq!(machine.cpu.status(), 0x24, "only I and the unused bit set");
}

#[test]
fn serial_status_and_data_behave_like_a_uart() {
    let mut machine = machine_with(0x0300, &[0xEA]);
    assert_eq!(machine.bus.peek(IO_SERIAL_STATUS), 0);
    machine.bus.push_serial_rx(b'X');
    assert_eq!(machine.bus.peek(IO_SERIAL_STATUS), 0x80);
    assert_eq!(
        machine.bus.peek(IO_SERIAL_DATA),
        b'X',
        "peeking does not consume"
    );
    assert_eq!(machine.bus.read(IO_SERIAL_DATA), b'X');
    assert_eq!(machine.bus.peek(IO_SERIAL_STATUS), 0);
    assert_eq!(machine.bus.read(IO_SERIAL_DATA), 0, "0 when nothing waits");
}

#[test]
fn serial_receive_buffer_holds_63_bytes() {
    let mut machine = machine_with(0x0300, &[0xEA]);
    for byte in 0..100 {
        machine.bus.push_serial_rx(byte);
    }
    let received: Vec<u8> = std::iter::from_fn(|| {
        (machine.bus.peek(IO_SERIAL_STATUS) != 0).then(|| machine.bus.read(IO_SERIAL_DATA))
    })
    .collect();
    assert_eq!(received, (0..63).collect::<Vec<u8>>());
}

#[test]
fn cpu_writes_to_serial_and_lcd_registers() {
    // LDA #'A'; STA LCD_DATA; STA SERIAL_DATA; JMP to self.
    let [lcd_lo, lcd_hi] = IO_LCD_DATA.to_le_bytes();
    let [serial_lo, serial_hi] = IO_SERIAL_DATA.to_le_bytes();
    let program = [
        0xA9, b'A', 0x8D, lcd_lo, lcd_hi, 0x8D, serial_lo, serial_hi, 0x4C, 0x08, 0x03,
    ];
    let mut machine = machine_with(0x0300, &program);
    run_until_stuck(&mut machine, 10);
    assert_eq!(machine.bus.lcd().text()[0][0], b'A');
    assert_eq!(machine.bus.pop_serial_tx(), Some(b'A'));
    assert_eq!(machine.bus.peek(VEC_RESET + 1), 0x03);
}

#[test]
fn lcd_text_wraps_and_moves_with_cr_and_lf() {
    let mut machine = machine_with(0x0300, &[0xEA]);
    for c in b"ABCDEFGHIJKLMNOPQRST" {
        machine.bus.poke(IO_LCD_DATA, *c);
    }
    assert_eq!(&machine.bus.lcd().text()[0], b"ABCDEFGHIJKLMNOP");
    assert_eq!(&machine.bus.lcd().text()[1], b"QRST            ");
    assert_eq!((machine.bus.lcd().row(), machine.bus.lcd().col()), (1, 4));
    machine.bus.poke(IO_LCD_DATA, b'\n');
    assert_eq!((machine.bus.lcd().row(), machine.bus.lcd().col()), (0, 0));
    machine.bus.poke(IO_LCD_DATA, b'x');
    machine.bus.poke(IO_LCD_DATA, b'\r');
    assert_eq!((machine.bus.lcd().row(), machine.bus.lcd().col()), (0, 0));
}

#[test]
fn nmos_decimal_mode() {
    // SED; CLC; LDA #$09; ADC #$01; STA $10; SEC; LDA #$10; SBC #$01; STA $11; JMP to self
    let program = [
        0xF8, 0x18, 0xA9, 0x09, 0x69, 0x01, 0x85, 0x10, 0x38, 0xA9, 0x10, 0xE9, 0x01, 0x85, 0x11,
        0x4C, 0x0F, 0x03,
    ];
    let mut machine = machine_with(0x0300, &program);
    run_until_stuck(&mut machine, 20);
    assert_eq!(machine.bus.peek(0x10), 0x10);
    assert_eq!(machine.bus.peek(0x11), 0x09);
}

#[test]
fn decimal_adc_takes_zero_flag_from_binary_sum() {
    // SED; CLC; LDA #$99; ADC #$01: BCD result $00, but the binary sum is $9A.
    let mut machine = machine_with(0x0300, &[0xF8, 0x18, 0xA9, 0x99, 0x69, 0x01]);
    for _ in 0..4 {
        machine.step();
    }
    assert_eq!(machine.cpu.a, 0x00);
    assert_eq!(
        machine.cpu.status() & 0x03,
        0x01,
        "C set, Z clear on an NMOS 6502"
    );
}

#[test]
fn unstable_opcodes_do_nothing_but_keep_their_length() {
    // LDA #$FF; LDX #$FF; LDY #$FF; SHY $0400,X ($9C); ANE #$00 ($8B); NOP
    let program = [
        0xA9, 0xFF, 0xA2, 0xFF, 0xA0, 0xFF, 0x9C, 0x00, 0x04, 0x8B, 0x00, 0xEA,
    ];
    let mut machine = machine_with(0x0300, &program);
    for _ in 0..5 {
        machine.step();
    }
    assert_eq!(machine.cpu.pc, 0x030B);
    assert_eq!(machine.cpu.a, 0xFF, "ANE left A alone");
    assert!(machine.bus.ram()[0x0400..0x0500].iter().all(|&b| b == 0));
}

#[test]
fn stable_undocumented_opcodes_work() {
    // LAX $10 ($A7) loads A and X; SAX $11 ($87) stores A & X.
    let mut machine = machine_with(0x0300, &[0xA7, 0x10, 0xA2, 0x0F, 0x87, 0x11]);
    machine.bus.poke(0x10, 0x5A);
    machine.step();
    assert_eq!((machine.cpu.a, machine.cpu.x), (0x5A, 0x5A));
    machine.step();
    machine.step();
    assert_eq!(machine.bus.peek(0x11), 0x0A);
}

#[test]
fn cycle_counts_include_page_crossing_and_branches() {
    // LDA $10FF,X with X=1 crosses a page: 5 cycles.  BEQ taken to the same
    // page: 3 cycles.
    let mut machine = machine_with(0x0300, &[0xA2, 0x01, 0xBD, 0xFF, 0x10, 0xF0, 0x00]);
    assert_eq!(machine.step(), 2);
    assert_eq!(machine.step(), 5);
    assert_eq!(machine.step(), 3);
}

fn disassembly(pc: u16, bytes: [u8; 3]) -> (u16, String) {
    let mut text = String::new();
    let len = disasm(pc, bytes, &mut text).expect("formats");
    (len, text)
}

#[test]
fn disassembler_formats_like_the_original() {
    assert_eq!(
        disassembly(0x0207, [0x8D, 0x00, 0xF0]),
        (3, "8D 00 F0  STA $F000  ; LCD_CONTROL".into())
    );
    assert_eq!(
        disassembly(0x050B, [0xD0, 0xF5, 0x00]),
        (2, "D0 F5     BNE $0502".into())
    );
    assert_eq!(
        disassembly(0x0300, [0xBD, 0x10, 0xF0]),
        (3, "BD 10 F0  LDA $F010,X  ; SERIAL_DATA".into())
    );
    assert_eq!(
        disassembly(0xFFFE, [0x10, 0x01, 0x00]),
        (2, "10 01     BPL $0001".into())
    );
    assert_eq!(
        disassembly(0x0200, [0x78, 0, 0]),
        (1, "78        SEI".into())
    );
    assert_eq!(
        disassembly(0x0200, [0xA9, 0x10, 0xF0]),
        (2, "A9 10     LDA #$10".into()),
        "immediate operands are never register names"
    );
}

#[test]
fn disassembler_shows_the_105_undocumented_opcodes_as_unknown() {
    let unknown = (0..=255u8)
        .filter(|&op| disassembly(0x1000, [op, 0x34, 0x12]).1.ends_with("???"))
        .count();
    assert_eq!(unknown, 105);
}

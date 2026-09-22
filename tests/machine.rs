//! Integration tests for the portable machine core.

use pico2_6502_rs::bus::{IO_LCD_DATA, IO_SERIAL_DATA, IO_SERIAL_STATUS, VEC_RESET};
use pico2_6502_rs::machine::Machine;
use pico2_6502_rs::monitor::Monitor;
use pico2_6502_rs::{DEMO_LOAD_ADDR, DEMO_PROGRAM};

#[test]
fn built_in_demo_writes_both_lcd_rows() {
    let mut machine = Machine::new();
    machine
        .cpu
        .memory
        .load(DEMO_LOAD_ADDR, DEMO_PROGRAM)
        .expect("demo should load");
    machine.reset_at(DEMO_LOAD_ADDR);

    for _ in 0..1_000 {
        let step = machine.step();
        if step.old_pc == step.new_pc {
            break;
        }
    }

    assert_eq!(&machine.cpu.memory.lcd()[0], b"RASPBERRY PICO 2");
    assert_eq!(&machine.cpu.memory.lcd()[1], b"HELLO FROM 6502 ");
}

#[test]
fn serial_status_and_data_have_uart_like_semantics() {
    let mut machine = Machine::new();
    assert_eq!(machine.cpu.memory.peek(IO_SERIAL_STATUS), 0);
    assert!(machine.cpu.memory.push_serial_rx(b'X'));
    assert_eq!(machine.cpu.memory.peek(IO_SERIAL_STATUS), 0x80);
    assert_eq!(machine.cpu.memory.peek(IO_SERIAL_DATA), b'X');
    assert_eq!(
        mos6502::memory::Bus::get_byte(&mut machine.cpu.memory, IO_SERIAL_DATA),
        b'X'
    );
    assert_eq!(machine.cpu.memory.peek(IO_SERIAL_STATUS), 0);
}

#[test]
fn cpu_can_write_to_serial_and_lcd_registers() {
    // LDA #'A'; STA LCD_DATA; STA SERIAL_DATA; JMP to self.
    let [lcd_lo, lcd_hi] = IO_LCD_DATA.to_le_bytes();
    let [serial_lo, serial_hi] = IO_SERIAL_DATA.to_le_bytes();
    let program = [
        0xA9, b'A', 0x8D, lcd_lo, lcd_hi, 0x8D, serial_lo, serial_hi, 0x4C, 0x08, 0x02,
    ];
    let mut machine = Machine::new();
    machine
        .cpu
        .memory
        .load(0x0200, &program)
        .expect("program should load");
    machine.reset_at(0x0200);
    for _ in 0..4 {
        let _ = machine.step();
    }
    assert_eq!(machine.cpu.memory.lcd()[0][0], b'A');
    assert_eq!(machine.cpu.memory.pop_serial_tx(), Some(b'A'));
    assert_eq!(machine.cpu.memory.peek(VEC_RESET), 0x00);
    assert_eq!(machine.cpu.memory.peek(VEC_RESET + 1), 0x02);
}

#[test]
fn monitor_reports_the_demo_display() {
    let mut machine = Machine::new();
    machine
        .cpu
        .memory
        .load(DEMO_LOAD_ADDR, DEMO_PROGRAM)
        .expect("demo should load");
    let mut monitor = Monitor::new();
    monitor.start_at(&mut machine, DEMO_LOAD_ADDR);
    while monitor.is_running() {
        let _ = monitor.tick(&mut machine);
    }
    for byte in b"d\n" {
        monitor.input(&mut machine, *byte);
    }
    let output = drain_monitor(&mut monitor);
    assert!(output.contains("|RASPBERRY PICO 2|"));
    assert!(output.contains("|HELLO FROM 6502 |"));
}

#[test]
fn monitor_loader_checks_crc_and_writes_ram() {
    let mut machine = Machine::new();
    let mut monitor = Monitor::new();
    for byte in b"l 0300 3 F508\nABC" {
        monitor.input(&mut machine, *byte);
    }
    assert_eq!(&machine.cpu.memory.ram()[0x0300..0x0303], b"ABC");
    let output = drain_monitor(&mut monitor);
    assert!(output.contains("READY"));
    assert!(output.contains("OK loaded 3 bytes"));
}

fn drain_monitor(monitor: &mut Monitor) -> String {
    let mut bytes = Vec::new();
    while let Some(byte) = monitor.pop_output() {
        bytes.push(byte);
    }
    String::from_utf8(bytes).expect("monitor output is ASCII")
}

//! Prints the disassembly of every opcode, placed at `$1000` with operand
//! bytes `$34 $12`, followed by a few fixed cases.  `host/test_disasm.py`
//! checks the output.

use pico2_6502_rs::disasm::disasm;

fn dump(pc: u16, bytes: [u8; 3]) {
    let mut text = String::new();
    let len = disasm(pc, bytes, &mut text).expect("formats");
    println!("{pc:04X} {len} {text}");
}

fn main() {
    for op in 0..=255 {
        dump(0x1000, [op, 0x34, 0x12]);
    }
    dump(0x0207, [0x8D, 0x00, 0xF0]); // STA $F000  ; LCD_CONTROL
    dump(0x050B, [0xD0, 0xF5, 0x00]); // BNE backwards
    dump(0x0300, [0xBD, 0x10, 0xF0]); // LDA $F010,X  ; SERIAL_DATA
    dump(0xFFFE, [0x10, 0x01, 0x00]); // BPL wrapping past $FFFF
}

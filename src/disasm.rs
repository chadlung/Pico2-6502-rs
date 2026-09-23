//! 6502 disassembler for the monitor's `r`, `s` and `u` commands.  Only the
//! documented NMOS opcodes are decoded; the others show as `???`.

use core::fmt::{self, Write};

use crate::bus::{
    IO_LCD_BACKLIGHT, IO_LCD_COL, IO_LCD_CONTROL, IO_LCD_DATA, IO_LCD_ROW, IO_SERIAL_DATA,
    IO_SERIAL_STATUS,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Imp,
    Acc,
    Imm,
    Zp,
    Zpx,
    Zpy,
    Abs,
    Abx,
    Aby,
    Ind,
    Izx,
    Izy,
    Rel,
}

impl Mode {
    const fn len(self) -> u16 {
        match self {
            Self::Imp | Self::Acc => 1,
            Self::Imm | Self::Zp | Self::Zpx | Self::Zpy | Self::Izx | Self::Izy | Self::Rel => 2,
            Self::Abs | Self::Abx | Self::Aby | Self::Ind => 3,
        }
    }
}

#[derive(Clone, Copy)]
struct Opcode {
    name: &'static str,
    mode: Mode,
}

const fn op(name: &'static str, mode: Mode) -> Opcode {
    Opcode { name, mode }
}

/// An undocumented opcode.
const UNKNOWN: Opcode = op("???", Mode::Imp);

#[rustfmt::skip]
static OPCODES: [Opcode; 256] = {
    use Mode::{Abs, Abx, Aby, Acc, Imm, Imp, Ind, Izx, Izy, Rel, Zp, Zpx, Zpy};
    [
        /* 00 */ op("BRK", Imp), op("ORA", Izx), UNKNOWN, UNKNOWN, UNKNOWN, op("ORA", Zp), op("ASL", Zp), UNKNOWN,
        /* 08 */ op("PHP", Imp), op("ORA", Imm), op("ASL", Acc), UNKNOWN, UNKNOWN, op("ORA", Abs), op("ASL", Abs), UNKNOWN,
        /* 10 */ op("BPL", Rel), op("ORA", Izy), UNKNOWN, UNKNOWN, UNKNOWN, op("ORA", Zpx), op("ASL", Zpx), UNKNOWN,
        /* 18 */ op("CLC", Imp), op("ORA", Aby), UNKNOWN, UNKNOWN, UNKNOWN, op("ORA", Abx), op("ASL", Abx), UNKNOWN,
        /* 20 */ op("JSR", Abs), op("AND", Izx), UNKNOWN, UNKNOWN, op("BIT", Zp), op("AND", Zp), op("ROL", Zp), UNKNOWN,
        /* 28 */ op("PLP", Imp), op("AND", Imm), op("ROL", Acc), UNKNOWN, op("BIT", Abs), op("AND", Abs), op("ROL", Abs), UNKNOWN,
        /* 30 */ op("BMI", Rel), op("AND", Izy), UNKNOWN, UNKNOWN, UNKNOWN, op("AND", Zpx), op("ROL", Zpx), UNKNOWN,
        /* 38 */ op("SEC", Imp), op("AND", Aby), UNKNOWN, UNKNOWN, UNKNOWN, op("AND", Abx), op("ROL", Abx), UNKNOWN,
        /* 40 */ op("RTI", Imp), op("EOR", Izx), UNKNOWN, UNKNOWN, UNKNOWN, op("EOR", Zp), op("LSR", Zp), UNKNOWN,
        /* 48 */ op("PHA", Imp), op("EOR", Imm), op("LSR", Acc), UNKNOWN, op("JMP", Abs), op("EOR", Abs), op("LSR", Abs), UNKNOWN,
        /* 50 */ op("BVC", Rel), op("EOR", Izy), UNKNOWN, UNKNOWN, UNKNOWN, op("EOR", Zpx), op("LSR", Zpx), UNKNOWN,
        /* 58 */ op("CLI", Imp), op("EOR", Aby), UNKNOWN, UNKNOWN, UNKNOWN, op("EOR", Abx), op("LSR", Abx), UNKNOWN,
        /* 60 */ op("RTS", Imp), op("ADC", Izx), UNKNOWN, UNKNOWN, UNKNOWN, op("ADC", Zp), op("ROR", Zp), UNKNOWN,
        /* 68 */ op("PLA", Imp), op("ADC", Imm), op("ROR", Acc), UNKNOWN, op("JMP", Ind), op("ADC", Abs), op("ROR", Abs), UNKNOWN,
        /* 70 */ op("BVS", Rel), op("ADC", Izy), UNKNOWN, UNKNOWN, UNKNOWN, op("ADC", Zpx), op("ROR", Zpx), UNKNOWN,
        /* 78 */ op("SEI", Imp), op("ADC", Aby), UNKNOWN, UNKNOWN, UNKNOWN, op("ADC", Abx), op("ROR", Abx), UNKNOWN,
        /* 80 */ UNKNOWN, op("STA", Izx), UNKNOWN, UNKNOWN, op("STY", Zp), op("STA", Zp), op("STX", Zp), UNKNOWN,
        /* 88 */ op("DEY", Imp), UNKNOWN, op("TXA", Imp), UNKNOWN, op("STY", Abs), op("STA", Abs), op("STX", Abs), UNKNOWN,
        /* 90 */ op("BCC", Rel), op("STA", Izy), UNKNOWN, UNKNOWN, op("STY", Zpx), op("STA", Zpx), op("STX", Zpy), UNKNOWN,
        /* 98 */ op("TYA", Imp), op("STA", Aby), op("TXS", Imp), UNKNOWN, UNKNOWN, op("STA", Abx), UNKNOWN, UNKNOWN,
        /* A0 */ op("LDY", Imm), op("LDA", Izx), op("LDX", Imm), UNKNOWN, op("LDY", Zp), op("LDA", Zp), op("LDX", Zp), UNKNOWN,
        /* A8 */ op("TAY", Imp), op("LDA", Imm), op("TAX", Imp), UNKNOWN, op("LDY", Abs), op("LDA", Abs), op("LDX", Abs), UNKNOWN,
        /* B0 */ op("BCS", Rel), op("LDA", Izy), UNKNOWN, UNKNOWN, op("LDY", Zpx), op("LDA", Zpx), op("LDX", Zpy), UNKNOWN,
        /* B8 */ op("CLV", Imp), op("LDA", Aby), op("TSX", Imp), UNKNOWN, op("LDY", Abx), op("LDA", Abx), op("LDX", Aby), UNKNOWN,
        /* C0 */ op("CPY", Imm), op("CMP", Izx), UNKNOWN, UNKNOWN, op("CPY", Zp), op("CMP", Zp), op("DEC", Zp), UNKNOWN,
        /* C8 */ op("INY", Imp), op("CMP", Imm), op("DEX", Imp), UNKNOWN, op("CPY", Abs), op("CMP", Abs), op("DEC", Abs), UNKNOWN,
        /* D0 */ op("BNE", Rel), op("CMP", Izy), UNKNOWN, UNKNOWN, UNKNOWN, op("CMP", Zpx), op("DEC", Zpx), UNKNOWN,
        /* D8 */ op("CLD", Imp), op("CMP", Aby), UNKNOWN, UNKNOWN, UNKNOWN, op("CMP", Abx), op("DEC", Abx), UNKNOWN,
        /* E0 */ op("CPX", Imm), op("SBC", Izx), UNKNOWN, UNKNOWN, op("CPX", Zp), op("SBC", Zp), op("INC", Zp), UNKNOWN,
        /* E8 */ op("INX", Imp), op("SBC", Imm), op("NOP", Imp), UNKNOWN, op("CPX", Abs), op("SBC", Abs), op("INC", Abs), UNKNOWN,
        /* F0 */ op("BEQ", Rel), op("SBC", Izy), UNKNOWN, UNKNOWN, UNKNOWN, op("SBC", Zpx), op("INC", Zpx), UNKNOWN,
        /* F8 */ op("SED", Imp), op("SBC", Aby), UNKNOWN, UNKNOWN, UNKNOWN, op("SBC", Abx), op("INC", Abx), UNKNOWN,
    ]
};

const fn register_name(address: u16) -> Option<&'static str> {
    match address {
        IO_LCD_CONTROL => Some("LCD_CONTROL"),
        IO_LCD_DATA => Some("LCD_DATA"),
        IO_LCD_ROW => Some("LCD_ROW"),
        IO_LCD_COL => Some("LCD_COL"),
        IO_LCD_BACKLIGHT => Some("LCD_BACKLIGHT"),
        IO_SERIAL_DATA => Some("SERIAL_DATA"),
        IO_SERIAL_STATUS => Some("SERIAL_STATUS"),
        _ => None,
    }
}

/// Formats the instruction at `pc`, whose first bytes are `bytes`, as for
/// example `8D 10 F0  STA $F010  ; SERIAL_DATA`, and returns its length.
///
/// # Errors
///
/// Returns [`fmt::Error`] when `out` rejects the text.
pub fn disasm(pc: u16, bytes: [u8; 3], out: &mut impl Write) -> Result<u16, fmt::Error> {
    let op = OPCODES[usize::from(bytes[0])];
    let len = op.mode.len();
    let byte = bytes[1];
    let word = u16::from_le_bytes([bytes[1], bytes[2]]);

    match len {
        1 => write!(out, "{:02X}      ", bytes[0])?,
        2 => write!(out, "{:02X} {:02X}   ", bytes[0], bytes[1])?,
        _ => write!(out, "{:02X} {:02X} {:02X}", bytes[0], bytes[1], bytes[2])?,
    }
    write!(out, "  {}", op.name)?;

    match op.mode {
        Mode::Imp => {}
        Mode::Acc => out.write_str(" A")?,
        Mode::Imm => write!(out, " #${byte:02X}")?,
        Mode::Zp => write!(out, " ${byte:02X}")?,
        Mode::Zpx => write!(out, " ${byte:02X},X")?,
        Mode::Zpy => write!(out, " ${byte:02X},Y")?,
        Mode::Abs => write!(out, " ${word:04X}")?,
        Mode::Abx => write!(out, " ${word:04X},X")?,
        Mode::Aby => write!(out, " ${word:04X},Y")?,
        Mode::Ind => write!(out, " (${word:04X})")?,
        Mode::Izx => write!(out, " (${byte:02X},X)")?,
        Mode::Izy => write!(out, " (${byte:02X}),Y")?,
        Mode::Rel => {
            let target = pc
                .wrapping_add(2)
                .wrapping_add_signed(i16::from(i8::from_ne_bytes([byte])));
            write!(out, " ${target:04X}")?;
        }
    }

    let named = matches!(op.mode, Mode::Abs | Mode::Abx | Mode::Aby | Mode::Ind);
    if let Some(name) = register_name(word).filter(|_| named) {
        write!(out, "  ; {name}")?;
    }
    Ok(len)
}

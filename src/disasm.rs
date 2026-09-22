//! Small monitor-oriented 6502 disassembler.

use core::fmt::{self, Write};

use mos6502::Variant;
use mos6502::instruction::{AddressingMode, Nmos6502};

use crate::bus::{
    IO_LCD_BACKLIGHT, IO_LCD_COL, IO_LCD_CONTROL, IO_LCD_DATA, IO_LCD_ROW, IO_SERIAL_DATA,
    IO_SERIAL_STATUS, MachineBus,
};

/// Disassembles one instruction and returns its byte length.
///
/// # Errors
///
/// Returns [`fmt::Error`] when the supplied writer rejects output.
pub fn disassemble(
    bus: &MachineBus,
    address: u16,
    output: &mut impl Write,
) -> Result<u16, fmt::Error> {
    let opcode = bus.peek(address);
    let Some((instruction, mode)) = Nmos6502::decode(opcode) else {
        write!(output, "{opcode:02X}        ???")?;
        return Ok(1);
    };
    let len = 1 + mode.extra_bytes();
    let byte = bus.peek(address.wrapping_add(1));
    let word = u16::from_le_bytes([byte, bus.peek(address.wrapping_add(2))]);

    match len {
        1 => write!(output, "{opcode:02X}        ")?,
        2 => write!(output, "{opcode:02X} {byte:02X}     ")?,
        _ => write!(output, "{opcode:02X} {byte:02X} {:02X}  ", word >> 8)?,
    }
    write!(output, "{instruction:?}")?;

    match mode {
        AddressingMode::Implied => {}
        AddressingMode::Accumulator => write!(output, " A")?,
        AddressingMode::Immediate => write!(output, " #${byte:02X}")?,
        AddressingMode::ZeroPage => write!(output, " ${byte:02X}")?,
        AddressingMode::ZeroPageX => write!(output, " ${byte:02X},X")?,
        AddressingMode::ZeroPageY => write!(output, " ${byte:02X},Y")?,
        AddressingMode::Relative => {
            let displacement = i8::from_ne_bytes([byte]);
            let target = address
                .wrapping_add(2)
                .wrapping_add_signed(i16::from(displacement));
            write!(output, " ${target:04X}")?;
        }
        AddressingMode::Absolute => write!(output, " ${word:04X}")?,
        AddressingMode::AbsoluteX => write!(output, " ${word:04X},X")?,
        AddressingMode::AbsoluteY => write!(output, " ${word:04X},Y")?,
        AddressingMode::Indirect | AddressingMode::BuggyIndirect => {
            write!(output, " (${word:04X})")?;
        }
        AddressingMode::IndexedIndirectX => write!(output, " (${byte:02X},X)")?,
        AddressingMode::IndirectIndexedY => write!(output, " (${byte:02X}),Y")?,
        _ => write!(output, " <{mode:?}>")?,
    }

    if let Some(name) = register_name(word) {
        write!(output, "  ; {name}")?;
    }
    Ok(len)
}

fn register_name(address: u16) -> Option<&'static str> {
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

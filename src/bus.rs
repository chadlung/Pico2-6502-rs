//! The emulated machine's address space: 64K of RAM with a page of
//! memory-mapped peripherals at `$F000-$F0FF`.

use heapless::Deque;

use crate::cpu::Memory;
use crate::lcd::{Lcd, LcdHardware};

/// First address of the memory-mapped I/O page.
pub const IO_PAGE: u16 = 0xF000;
/// W: `$01` clear display and home cursor, `$02` home cursor.
pub const IO_LCD_CONTROL: u16 = 0xF000;
/// W: print a character and advance the cursor.
pub const IO_LCD_DATA: u16 = 0xF001;
/// R/W: cursor row 0-1.
pub const IO_LCD_ROW: u16 = 0xF002;
/// R/W: cursor column 0-15.
pub const IO_LCD_COL: u16 = 0xF003;
/// R/W: 0 = backlight off, anything else = on.
pub const IO_LCD_BACKLIGHT: u16 = 0xF004;
/// W: send a byte over USB serial; R: next received byte (0 if none).
pub const IO_SERIAL_DATA: u16 = 0xF010;
/// R: bit 7 set when a received byte is waiting.
pub const IO_SERIAL_STATUS: u16 = 0xF011;

/// Non-maskable interrupt vector.
pub const VEC_NMI: u16 = 0xFFFA;
/// Reset vector.
pub const VEC_RESET: u16 = 0xFFFC;
/// Interrupt request and `BRK` vector.
pub const VEC_IRQ: u16 = 0xFFFE;

/// The 6502's memory, its serial port and its display.
pub struct MachineBus<H> {
    ram: [u8; 65_536],
    /// Bytes waiting for the 6502, like a UART receive buffer.
    serial_rx: Deque<u8, 63>,
    /// Bytes the 6502 has sent, until the monitor passes them on.
    serial_tx: Deque<u8, 16>,
    lcd: Lcd<H>,
}

impl<H: LcdHardware> MachineBus<H> {
    /// Creates zeroed RAM with the given display.  Call
    /// [`Lcd::init`] through [`lcd_mut`](Self::lcd_mut) before use.
    #[allow(clippy::large_stack_arrays)]
    pub const fn new(display: H) -> Self {
        Self {
            ram: [0; 65_536],
            serial_rx: Deque::new(),
            serial_tx: Deque::new(),
            lcd: Lcd::new(display),
        }
    }

    /// Zeroes all RAM and empties the serial receive buffer.
    pub fn reset_memory(&mut self) {
        self.ram.fill(0);
        self.clear_serial_rx();
    }

    /// Reads without side effects, for the monitor.
    #[must_use]
    #[cfg_attr(
        feature = "firmware",
        allow(unsafe_code),
        unsafe(link_section = ".data.ram_func")
    )]
    pub fn peek(&self, address: u16) -> u8 {
        if is_io(address) {
            self.io_read(address)
        } else {
            self.ram[usize::from(address)]
        }
    }

    /// Writes through the memory map, as a 6502 store would: I/O registers
    /// take effect.
    pub fn poke(&mut self, address: u16, value: u8) {
        self.write(address, value);
    }

    /// Copies bytes straight into RAM.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError::OutOfRange`] when the bytes run past `$FFFF`,
    /// or [`LoadError::IoOverlap`] when they cover the I/O page.
    pub fn load(&mut self, start: u16, bytes: &[u8]) -> Result<(), LoadError> {
        let start = usize::from(start);
        let end = start + bytes.len();
        if end > self.ram.len() {
            return Err(LoadError::OutOfRange);
        }
        if start < usize::from(IO_PAGE) + 0x100 && end > usize::from(IO_PAGE) {
            return Err(LoadError::IoOverlap);
        }
        self.ram[start..end].copy_from_slice(bytes);
        Ok(())
    }

    /// The RAM behind the whole address space, including the unused bytes
    /// under the I/O page.
    #[must_use]
    pub const fn ram(&self) -> &[u8; 65_536] {
        &self.ram
    }

    /// Mutable RAM, bypassing the I/O page.
    pub const fn ram_mut(&mut self) -> &mut [u8; 65_536] {
        &mut self.ram
    }

    /// Queues a byte for the 6502; it is dropped when the buffer is full.
    pub fn push_serial_rx(&mut self, byte: u8) {
        let _ = self.serial_rx.push_back(byte);
    }

    /// Empties the serial receive buffer.
    pub fn clear_serial_rx(&mut self) {
        self.serial_rx.clear();
    }

    /// Takes the next byte the 6502 wrote to `SERIAL_DATA`.
    pub fn pop_serial_tx(&mut self) -> Option<u8> {
        self.serial_tx.pop_front()
    }

    /// Whether the 6502 has written bytes not yet taken.
    #[must_use]
    pub fn has_serial_tx(&self) -> bool {
        !self.serial_tx.is_empty()
    }

    /// The display.
    pub const fn lcd(&self) -> &Lcd<H> {
        &self.lcd
    }

    /// The display, mutably.
    pub const fn lcd_mut(&mut self) -> &mut Lcd<H> {
        &mut self.lcd
    }

    fn io_read(&self, address: u16) -> u8 {
        match address {
            IO_LCD_ROW => self.lcd.row(),
            IO_LCD_COL => self.lcd.col(),
            IO_LCD_BACKLIGHT => u8::from(self.lcd.backlight_on()),
            IO_SERIAL_STATUS if !self.serial_rx.is_empty() => 0x80,
            IO_SERIAL_DATA => self.serial_rx.front().copied().unwrap_or(0),
            _ => 0,
        }
    }

    fn io_write(&mut self, address: u16, value: u8) {
        match address {
            IO_LCD_CONTROL if value & 0x01 != 0 => self.lcd.clear(),
            IO_LCD_CONTROL if value & 0x02 != 0 => self.lcd.home(),
            IO_LCD_DATA => self.lcd.putc(value),
            IO_LCD_ROW => self.lcd.set_row(value),
            IO_LCD_COL => self.lcd.set_col(value),
            IO_LCD_BACKLIGHT => self.lcd.set_backlight(value != 0),
            IO_SERIAL_DATA => {
                let _ = self.serial_tx.push_back(value);
            }
            _ => {}
        }
    }
}

// The CPU calls these for every access, so the firmware runs them from RAM.
impl<H: LcdHardware> Memory for MachineBus<H> {
    #[cfg_attr(
        feature = "firmware",
        allow(unsafe_code),
        unsafe(link_section = ".data.ram_func")
    )]
    fn read(&mut self, address: u16) -> u8 {
        if address == IO_SERIAL_DATA {
            self.serial_rx.pop_front().unwrap_or(0)
        } else {
            self.peek(address)
        }
    }

    #[cfg_attr(
        feature = "firmware",
        allow(unsafe_code),
        unsafe(link_section = ".data.ram_func")
    )]
    fn write(&mut self, address: u16, value: u8) {
        if is_io(address) {
            self.io_write(address, value);
        } else {
            self.ram[usize::from(address)] = value;
        }
    }
}

/// Error from [`MachineBus::load`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// The bytes run past `$FFFF`.
    OutOfRange,
    /// The bytes cover the I/O page `$F000-$F0FF`.
    IoOverlap,
}

/// Whether an address is in the I/O page.
#[must_use]
pub const fn is_io(address: u16) -> bool {
    address & 0xFF00 == IO_PAGE
}

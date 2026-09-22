//! Memory map and emulated peripherals.

use heapless::Deque;
use mos6502::memory::Bus;

/// First address of the memory-mapped I/O page.
pub const IO_PAGE: u16 = 0xF000;
/// LCD command register.
pub const IO_LCD_CONTROL: u16 = 0xF000;
/// LCD character register.
pub const IO_LCD_DATA: u16 = 0xF001;
/// LCD cursor row register.
pub const IO_LCD_ROW: u16 = 0xF002;
/// LCD cursor column register.
pub const IO_LCD_COL: u16 = 0xF003;
/// LCD backlight register.
pub const IO_LCD_BACKLIGHT: u16 = 0xF004;
/// Serial receive/transmit register.
pub const IO_SERIAL_DATA: u16 = 0xF010;
/// Serial receive-ready register.
pub const IO_SERIAL_STATUS: u16 = 0xF011;

/// Non-maskable interrupt vector.
pub const VEC_NMI: u16 = 0xFFFA;
/// Reset vector.
pub const VEC_RESET: u16 = 0xFFFC;
/// Interrupt/BRK vector.
pub const VEC_IRQ: u16 = 0xFFFE;

/// Number of LCD rows.
pub const LCD_ROWS: usize = 2;
/// Number of LCD columns.
pub const LCD_COLS: usize = 16;
const LCD_ROWS_U8: u8 = 2;
const LCD_COLS_U8: u8 = 16;

/// A pending physical-display operation created by a 6502 bus write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LcdEvent {
    /// Clear the display.
    Clear,
    /// Move to the home position.
    Home,
    /// Move the cursor.
    Position {
        /// Zero-based row.
        row: u8,
        /// Zero-based column.
        col: u8,
    },
    /// Write one character.
    Write(u8),
    /// Change the backlight.
    Backlight(bool),
}

/// Complete 6502 address space and the state of its memory-mapped devices.
pub struct MachineBus {
    ram: [u8; 65_536],
    serial_rx: Deque<u8, 64>,
    serial_tx: Deque<u8, 256>,
    lcd_events: Deque<LcdEvent, 32>,
    lcd: [[u8; LCD_COLS]; LCD_ROWS],
    row: u8,
    col: u8,
    backlight: bool,
    display_address: Option<u8>,
}

impl Default for MachineBus {
    fn default() -> Self {
        Self::new()
    }
}

impl MachineBus {
    /// Creates a zero-filled address space with a blank LCD.
    #[must_use]
    #[allow(clippy::large_stack_arrays)]
    pub const fn new() -> Self {
        Self {
            ram: [0; 65_536],
            serial_rx: Deque::new(),
            serial_tx: Deque::new(),
            lcd_events: Deque::new(),
            lcd: [[b' '; LCD_COLS]; LCD_ROWS],
            row: 0,
            col: 0,
            backlight: true,
            display_address: None,
        }
    }

    /// Clears RAM and queued serial input/output.
    pub fn reset_memory(&mut self) {
        self.ram.fill(0);
        self.serial_rx.clear();
        self.serial_tx.clear();
    }

    /// Reads memory without triggering I/O side effects.
    #[must_use]
    pub fn peek(&self, address: u16) -> u8 {
        match address {
            IO_LCD_ROW => self.row,
            IO_LCD_COL => self.col,
            IO_LCD_BACKLIGHT => u8::from(self.backlight),
            IO_SERIAL_STATUS => {
                if self.serial_rx.is_empty() {
                    0
                } else {
                    0x80
                }
            }
            IO_SERIAL_DATA => self.serial_rx.front().copied().unwrap_or(0),
            _ if is_io(address) => 0,
            _ => self.ram[usize::from(address)],
        }
    }

    /// Writes a byte through the normal memory map.
    pub fn poke(&mut self, address: u16, value: u8) {
        self.set_byte(address, value);
    }

    /// Writes directly to RAM, bypassing memory-mapped I/O.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError::OutOfRange`] when the range exceeds the address
    /// space, or [`LoadError::IoOverlap`] when it covers the I/O page.
    pub fn load(&mut self, start: u16, bytes: &[u8]) -> Result<(), LoadError> {
        let start = usize::from(start);
        let end = start
            .checked_add(bytes.len())
            .ok_or(LoadError::OutOfRange)?;
        if end > self.ram.len() {
            return Err(LoadError::OutOfRange);
        }
        if start < usize::from(IO_PAGE + 0x100) && end > usize::from(IO_PAGE) {
            return Err(LoadError::IoOverlap);
        }
        self.ram[start..end].copy_from_slice(bytes);
        Ok(())
    }

    /// Returns the underlying RAM image for inspection or persistence.
    #[must_use]
    pub const fn ram(&self) -> &[u8; 65_536] {
        &self.ram
    }

    /// Adds one byte to the emulated serial receive FIFO.
    pub fn push_serial_rx(&mut self, byte: u8) -> bool {
        self.serial_rx.push_back(byte).is_ok()
    }

    /// Removes one byte written by the 6502 to the serial data register.
    pub fn pop_serial_tx(&mut self) -> Option<u8> {
        self.serial_tx.pop_front()
    }

    /// Removes the next pending physical LCD operation.
    pub fn pop_lcd_event(&mut self) -> Option<LcdEvent> {
        self.lcd_events.pop_front()
    }

    /// Returns the LCD text mirror.
    #[must_use]
    pub const fn lcd(&self) -> &[[u8; LCD_COLS]; LCD_ROWS] {
        &self.lcd
    }

    /// Returns the logical cursor position.
    #[must_use]
    pub const fn lcd_cursor(&self) -> (u8, u8) {
        (self.row, self.col)
    }

    /// Returns whether the logical LCD backlight is enabled.
    #[must_use]
    pub const fn lcd_backlight(&self) -> bool {
        self.backlight
    }

    /// Records the detected PCF8574 address, or `None` when absent.
    pub const fn set_display_address(&mut self, address: Option<u8>) {
        self.display_address = address;
    }

    /// Returns the detected PCF8574 address.
    #[must_use]
    pub const fn display_address(&self) -> Option<u8> {
        self.display_address
    }

    fn queue_lcd(&mut self, event: LcdEvent) {
        let _ = self.lcd_events.push_back(event);
    }

    fn clear_lcd(&mut self) {
        self.lcd = [[b' '; LCD_COLS]; LCD_ROWS];
        self.row = 0;
        self.col = 0;
        self.queue_lcd(LcdEvent::Clear);
    }

    fn write_lcd(&mut self, byte: u8) {
        match byte {
            b'\r' => self.col = 0,
            b'\n' => {
                self.row = (self.row + 1) % LCD_ROWS_U8;
                self.col = 0;
            }
            _ => {
                self.lcd[usize::from(self.row)][usize::from(self.col)] = byte;
                self.queue_lcd(LcdEvent::Position {
                    row: self.row,
                    col: self.col,
                });
                self.queue_lcd(LcdEvent::Write(byte));
                self.col += 1;
                if usize::from(self.col) == LCD_COLS {
                    self.row = (self.row + 1) % LCD_ROWS_U8;
                    self.col = 0;
                }
            }
        }
    }
}

impl Bus for MachineBus {
    fn get_byte(&mut self, address: u16) -> u8 {
        if address == IO_SERIAL_DATA {
            self.serial_rx.pop_front().unwrap_or(0)
        } else {
            self.peek(address)
        }
    }

    fn set_byte(&mut self, address: u16, value: u8) {
        match address {
            IO_LCD_CONTROL if value & 0x01 != 0 => self.clear_lcd(),
            IO_LCD_CONTROL if value & 0x02 != 0 => {
                self.row = 0;
                self.col = 0;
                self.queue_lcd(LcdEvent::Home);
            }
            IO_LCD_CONTROL => {}
            IO_LCD_DATA => self.write_lcd(value),
            IO_LCD_ROW => {
                self.row = value % LCD_ROWS_U8;
                self.queue_lcd(LcdEvent::Position {
                    row: self.row,
                    col: self.col,
                });
            }
            IO_LCD_COL => {
                self.col = value % LCD_COLS_U8;
                self.queue_lcd(LcdEvent::Position {
                    row: self.row,
                    col: self.col,
                });
            }
            IO_LCD_BACKLIGHT => {
                self.backlight = value != 0;
                self.queue_lcd(LcdEvent::Backlight(self.backlight));
            }
            IO_SERIAL_DATA => {
                let _ = self.serial_tx.push_back(value);
            }
            _ if is_io(address) => {}
            _ => self.ram[usize::from(address)] = value,
        }
    }
}

/// Error returned when loading bytes directly into RAM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// The byte range lies outside the 16-bit address space.
    OutOfRange,
    /// The byte range overlaps `$F000-$F0FF`.
    IoOverlap,
}

/// Returns whether an address belongs to the memory-mapped I/O page.
#[must_use]
pub const fn is_io(address: u16) -> bool {
    address & 0xFF00 == IO_PAGE
}

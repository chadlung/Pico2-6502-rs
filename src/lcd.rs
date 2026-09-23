//! 16x2 character display as seen by 6502 programs.
//!
//! The cursor is tracked here so text wraps from the end of one row to the
//! start of the next (the HD44780 does not do this on its own).  Every
//! character also lands in a text mirror, which lets the monitor show the
//! screen and lets the simulator work with no display at all.

use core::fmt;

/// Characters per row.
pub const LCD_COLS: usize = 16;
/// Rows on the display.
pub const LCD_ROWS: usize = 2;

#[allow(clippy::cast_possible_truncation)]
const ROWS: u8 = LCD_ROWS as u8;
#[allow(clippy::cast_possible_truncation)]
const COLS: u8 = LCD_COLS as u8;

const HD44780_CLEAR: u8 = 0x01;
const HD44780_SET_DDRAM: u8 = 0x80;
const ROW_START: [u8; LCD_ROWS] = [0x00, 0x40];

/// Low-level HD44780 access.  The firmware implements this for the Freenove
/// I2C LCD1602's PCF8574 backpack; the simulator uses [`NoDisplay`].
pub trait LcdHardware {
    /// Looks for the display and initialises it.  Returns `false` if none
    /// answers.  May be called again.
    fn init(&mut self) -> bool;
    /// Writes the I2C line levels and the devices that answer.
    fn diagnose(&mut self, out: &mut dyn fmt::Write);
    /// The I2C address found by [`init`](Self::init), if any.
    fn address(&self) -> Option<u8>;
    /// Sends an instruction byte (RS low).
    fn command(&mut self, command: u8);
    /// Sends a character byte (RS high).
    fn write(&mut self, data: u8);
    /// Switches the backlight.
    fn backlight(&mut self, on: bool);
}

/// No display: only the text mirror exists.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoDisplay;

impl LcdHardware for NoDisplay {
    fn init(&mut self) -> bool {
        false
    }

    fn diagnose(&mut self, out: &mut dyn fmt::Write) {
        let _ = out.write_str("The simulator has no I2C bus.\n");
    }

    fn address(&self) -> Option<u8> {
        None
    }

    fn command(&mut self, _command: u8) {}

    fn write(&mut self, _data: u8) {}

    fn backlight(&mut self, _on: bool) {}
}

/// The display's logical state and its hardware.
pub struct Lcd<H> {
    hardware: H,
    present: bool,
    backlight: bool,
    row: u8,
    col: u8,
    /// The display's cursor must be repositioned before the next character.
    cursor_moved: bool,
    mirror: [[u8; LCD_COLS]; LCD_ROWS],
}

impl<H: LcdHardware> Lcd<H> {
    /// Wraps the hardware.  Call [`init`](Self::init) before use.
    pub const fn new(hardware: H) -> Self {
        Self {
            hardware,
            present: false,
            backlight: false,
            row: 0,
            col: 0,
            cursor_moved: false,
            mirror: [[b' '; LCD_COLS]; LCD_ROWS],
        }
    }

    /// (Re)detects the display, switches the backlight on and clears it.
    pub fn init(&mut self) {
        self.present = self.hardware.init();
        self.set_backlight(true);
        self.clear();
    }

    /// Writes I2C details for troubleshooting.
    pub fn diagnose(&mut self, out: &mut dyn fmt::Write) {
        self.hardware.diagnose(out);
    }

    /// Whether a display answered at the last [`init`](Self::init).
    pub const fn present(&self) -> bool {
        self.present
    }

    /// The display's I2C address, if one was found.
    pub fn address(&self) -> Option<u8> {
        self.hardware.address()
    }

    /// Blanks the screen and moves the cursor to row 0, column 0.
    pub fn clear(&mut self) {
        self.mirror = [[b' '; LCD_COLS]; LCD_ROWS];
        self.row = 0;
        self.col = 0;
        self.cursor_moved = false;
        if self.present {
            self.hardware.command(HD44780_CLEAR); // also homes the cursor
        }
    }

    /// Moves the cursor to row 0, column 0.
    pub const fn home(&mut self) {
        self.row = 0;
        self.col = 0;
        self.cursor_moved = true;
    }

    const fn next_row(&mut self) {
        self.col = 0;
        self.row = (self.row + 1) % ROWS;
        self.cursor_moved = true;
    }

    /// Prints a character and advances.  CR goes to column 0; LF goes to
    /// column 0 of the other row.
    pub fn putc(&mut self, c: u8) {
        match c {
            b'\r' => {
                self.col = 0;
                self.cursor_moved = true;
            }
            b'\n' => self.next_row(),
            _ => {
                if self.present {
                    if self.cursor_moved {
                        let address = ROW_START[usize::from(self.row)] + self.col;
                        self.hardware.command(HD44780_SET_DDRAM | address);
                    }
                    self.hardware.write(c);
                }
                self.cursor_moved = false;
                self.mirror[usize::from(self.row)][usize::from(self.col)] = c;
                self.col += 1;
                if self.col == COLS {
                    self.next_row();
                }
            }
        }
    }

    /// Moves the cursor to `row`, taken modulo the number of rows.
    pub const fn set_row(&mut self, row: u8) {
        self.row = row % ROWS;
        self.cursor_moved = true;
    }

    /// Moves the cursor to `col`, taken modulo the number of columns.
    pub const fn set_col(&mut self, col: u8) {
        self.col = col % COLS;
        self.cursor_moved = true;
    }

    /// The cursor row.
    pub const fn row(&self) -> u8 {
        self.row
    }

    /// The cursor column.
    pub const fn col(&self) -> u8 {
        self.col
    }

    /// Switches the backlight.
    pub fn set_backlight(&mut self, on: bool) {
        self.backlight = on;
        if self.present {
            self.hardware.backlight(on);
        }
    }

    /// Whether the backlight is on.
    pub const fn backlight_on(&self) -> bool {
        self.backlight
    }

    /// The text on the screen.
    pub const fn text(&self) -> &[[u8; LCD_COLS]; LCD_ROWS] {
        &self.mirror
    }

    /// The hardware, for example to inspect it in tests.
    pub const fn hardware(&self) -> &H {
        &self.hardware
    }
}

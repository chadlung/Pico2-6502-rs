//! The complete emulated computer: CPU, memory and devices.

use crate::bus::{MachineBus, VEC_RESET};
use crate::cpu::Cpu;
use crate::lcd::LcdHardware;

/// The 6502, its memory and its devices.
pub struct Machine<H> {
    /// CPU registers and flags.
    pub cpu: Cpu,
    /// Memory and devices.
    pub bus: MachineBus<H>,
}

impl<H: LcdHardware> Machine<H> {
    /// Creates a machine with zeroed RAM and the given display.  Call
    /// [`init`](Self::init) before use.
    pub const fn new(display: H) -> Self {
        Self {
            cpu: Cpu::new(),
            bus: MachineBus::new(display),
        }
    }

    /// Zeroes RAM and looks for the display, as at power-up.
    pub fn init(&mut self) {
        self.bus.reset_memory();
        self.bus.lcd_mut().init();
    }

    /// Resets the CPU from the reset vector.
    pub fn reset(&mut self) {
        self.cpu.reset(&mut self.bus);
    }

    /// Executes one instruction and returns the cycles it took.
    pub fn step(&mut self) -> u32 {
        self.cpu.step(&mut self.bus)
    }

    /// Reads a little-endian word, such as a vector, without side effects.
    #[must_use]
    pub fn word(&self, address: u16) -> u16 {
        u16::from_le_bytes([
            self.bus.peek(address),
            self.bus.peek(address.wrapping_add(1)),
        ])
    }

    /// Stores `address` in the reset vector.
    pub fn set_reset_vector(&mut self, address: u16) {
        let [lo, hi] = address.to_le_bytes();
        let ram = self.bus.ram_mut();
        ram[usize::from(VEC_RESET)] = lo;
        ram[usize::from(VEC_RESET) + 1] = hi;
    }
}

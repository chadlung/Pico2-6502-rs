//! CPU and bus orchestration.

use mos6502::cpu::{CPU, WaitState};
use mos6502::instruction::Nmos6502;

use crate::bus::{MachineBus, VEC_IRQ, VEC_RESET};

/// Result of executing one emulated instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepResult {
    /// Program counter before the instruction.
    pub old_pc: u16,
    /// Program counter after the instruction.
    pub new_pc: u16,
    /// Cycles used by the instruction.
    pub cycles: u64,
    /// Whether the CPU core executed an instruction.
    pub executed: bool,
}

/// The complete emulated computer.
pub struct Machine {
    /// The underlying NMOS 6502 core and memory bus.
    pub cpu: CPU<MachineBus, Nmos6502>,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    /// Creates a new machine with zero-filled RAM.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cpu: CPU::new(MachineBus::new(), Nmos6502),
        }
    }

    /// Resets the CPU from its reset vector.
    pub fn reset(&mut self) {
        self.cpu.reset();
    }

    /// Makes `address` the reset target and resets the CPU.
    pub fn reset_at(&mut self, address: u16) {
        let [lo, hi] = address.to_le_bytes();
        self.cpu.memory.poke(VEC_RESET, lo);
        self.cpu.memory.poke(VEC_RESET.wrapping_add(1), hi);
        self.reset();
    }

    /// Executes one instruction or one waiting-state cycle.
    #[must_use]
    pub fn step(&mut self) -> StepResult {
        let old_pc = self.cpu.registers.program_counter;
        let old_cycles = self.cpu.cycles;
        let executed = self.cpu.single_step();
        StepResult {
            old_pc,
            new_pc: self.cpu.registers.program_counter,
            cycles: self.cpu.cycles.wrapping_sub(old_cycles),
            executed,
        }
    }

    /// Returns `true` when the next instruction is an unhandled `BRK`.
    #[must_use]
    pub fn has_unhandled_brk(&self) -> bool {
        let pc = self.cpu.registers.program_counter;
        self.cpu.memory.peek(pc) == 0
            && u16::from_le_bytes([
                self.cpu.memory.peek(VEC_IRQ),
                self.cpu.memory.peek(VEC_IRQ.wrapping_add(1)),
            ]) == 0
    }

    /// Returns whether the emulated CPU is halted until reset.
    #[must_use]
    pub const fn is_stopped(&self) -> bool {
        matches!(self.cpu.wait_state(), WaitState::WaitingForReset)
    }
}

//! Portable core of a small 6502 computer: 64K of RAM, a 16x2 LCD and a
//! byte-oriented serial port, with a monitor for loading and debugging
//! programs.  The Pico 2 firmware and the PC simulator share all of it.

#![no_std]

pub mod bus;
pub mod cpu;
mod demo_program;
pub mod disasm;
pub mod lcd;
pub mod machine;
pub mod monitor;
pub mod platform;

pub use demo_program::{DEMO_LOAD_ADDR, DEMO_PROGRAM};

//! Services each front end provides to the monitor: the Pico firmware
//! (`src/bin/pico6502.rs`) and the simulator (`src/bin/sim6502.rs`).
//!
//! The methods are `async` so the firmware can wait on USB without blocking
//! its other tasks; the simulator's versions simply block.

/// Terminal, clock and LED.
// The front ends are single-threaded, so the futures need no `Send` bound.
#[allow(async_fn_in_trait)]
pub trait Platform {
    /// Returns the next input byte, waiting up to `timeout_us`, or `None` on
    /// timeout.  Sends any buffered output first.
    async fn getc(&mut self, timeout_us: u32) -> Option<u8>;
    /// Writes bytes to the terminal, possibly buffering them.
    async fn write(&mut self, bytes: &[u8]);
    /// Sends buffered output.
    async fn flush(&mut self);
    /// A monotonic clock in microseconds.
    fn time_us(&mut self) -> u64;
    /// Waits `us` microseconds.  Sends any buffered output first.
    async fn sleep_us(&mut self, us: u64);
    /// Lit while a 6502 program runs.
    fn led(&mut self, on: bool);
}

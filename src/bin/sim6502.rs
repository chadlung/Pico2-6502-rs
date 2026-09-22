//! Desktop front end for the portable emulator core.

use std::io::{self, Read, Write};
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use pico2_6502_rs::machine::Machine;
use pico2_6502_rs::monitor::Monitor;
use pico2_6502_rs::{DEMO_LOAD_ADDR, DEMO_PROGRAM};

fn main() -> io::Result<()> {
    let mut machine = Machine::new();
    machine
        .cpu
        .memory
        .load(DEMO_LOAD_ADDR, DEMO_PROGRAM)
        .expect("built-in demo fits in RAM");
    machine.reset_at(DEMO_LOAD_ADDR);

    let mut monitor = Monitor::new();
    monitor.init(&machine);
    monitor.start_at(&mut machine, DEMO_LOAD_ADDR);
    while monitor.is_running() {
        let _ = monitor.tick(&mut machine);
    }

    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for byte in io::stdin().lock().bytes() {
            match byte {
                Ok(byte) if sender.send(byte).is_ok() => {}
                _ => break,
            }
        }
    });

    let mut stdout = io::stdout().lock();
    let mut deadline = Instant::now();
    let mut input_closed = false;
    loop {
        loop {
            match receiver.try_recv() {
                Ok(byte) => monitor.input(&mut machine, byte),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    input_closed = true;
                    break;
                }
            }
        }

        if monitor.is_running() {
            let cycles = monitor.tick(&mut machine);
            if monitor.speed_khz() != 0 && cycles != 0 {
                deadline += Duration::from_nanos(
                    cycles.saturating_mul(1_000_000) / u64::from(monitor.speed_khz()),
                );
                let now = Instant::now();
                if deadline > now {
                    thread::sleep(deadline - now);
                } else if now.duration_since(deadline) > Duration::from_millis(100) {
                    deadline = now;
                }
            }
        } else {
            deadline = Instant::now();
            thread::sleep(Duration::from_millis(1));
        }

        while let Some(byte) = machine.cpu.memory.pop_serial_tx() {
            stdout.write_all(&[byte])?;
        }
        while let Some(byte) = monitor.pop_output() {
            stdout.write_all(&[byte])?;
        }
        stdout.flush()?;
        if input_closed && !monitor.is_running() {
            return Ok(());
        }
    }
}

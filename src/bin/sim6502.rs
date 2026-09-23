//! sim6502: the Pico 6502 computer running on a PC.
//!
//! The monitor, bus and CPU are the same code the Pico runs.  This terminal
//! (or, with `-p`, a virtual serial port for `tools/upload.py`) stands in for
//! USB serial, and the LCD exists only as the text shown by the monitor's `d`
//! command.
//!
//! With input from a pipe, the simulator lets the startup program finish (for
//! up to a second) before reading commands, so they are not taken as input
//! to the program.  At the end of the input it lets a running program finish,
//! then exits.

use std::ffi::CStr;
use std::io::{self, BufWriter, Stdout, Write as _};
use std::process;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use embassy_futures::block_on;
use pico2_6502_rs::lcd::NoDisplay;
use pico2_6502_rs::machine::Machine;
use pico2_6502_rs::monitor::Monitor;
use pico2_6502_rs::platform::Platform;
use pico2_6502_rs::{DEMO_LOAD_ADDR, DEMO_PROGRAM};

const CTRL_RIGHT_BRACKET: u8 = 0x1D;

/// The terminal settings to restore on exit, once raw mode is on.
static SAVED_TERMIOS: OnceLock<libc::termios> = OnceLock::new();

/// The monitor's terminal: standard input and output.
struct Terminal {
    out: BufWriter<Stdout>,
    start: Instant,
    raw: bool,
    /// Leave input unread, for a pipe while the startup program runs.
    hold_input: bool,
    input_closed: bool,
}

// The terminal blocks, so these async functions never wait.
#[allow(clippy::unused_async_trait_impl)]
impl Platform for Terminal {
    async fn getc(&mut self, timeout_us: u32) -> Option<u8> {
        let _ = self.out.flush();
        if self.input_closed || self.hold_input {
            return None;
        }
        let wait_ms = i32::try_from(timeout_us.div_ceil(1000)).unwrap_or(i32::MAX);
        let byte = read_stdin(wait_ms);
        match byte {
            Input::Byte(CTRL_RIGHT_BRACKET) if self.raw => quit(0),
            Input::Byte(byte) => Some(byte),
            Input::Closed => {
                self.input_closed = true;
                None
            }
            Input::None => None,
        }
    }

    async fn write(&mut self, bytes: &[u8]) {
        let _ = self.out.write_all(bytes);
    }

    async fn flush(&mut self) {
        let _ = self.out.flush();
    }

    fn time_us(&mut self) -> u64 {
        u64::try_from(self.start.elapsed().as_micros()).unwrap_or(u64::MAX)
    }

    async fn sleep_us(&mut self, us: u64) {
        let _ = self.out.flush();
        thread::sleep(Duration::from_micros(us));
    }

    fn led(&mut self, _on: bool) {}
}

enum Input {
    Byte(u8),
    None,
    Closed,
}

#[allow(unsafe_code)]
fn read_stdin(timeout_ms: i32) -> Input {
    let mut poll = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: `poll` is one valid pollfd for the duration of the call.
    if unsafe { libc::poll(&raw mut poll, 1, timeout_ms) } <= 0 {
        return Input::None;
    }
    let mut byte = 0u8;
    // SAFETY: reads at most one byte into `byte`.
    match unsafe { libc::read(libc::STDIN_FILENO, (&raw mut byte).cast(), 1) } {
        1 => Input::Byte(byte),
        0 => Input::Closed,
        _ => Input::None,
    }
}

/// Restores the terminal and exits.
fn quit(code: i32) -> ! {
    restore_terminal();
    process::exit(code);
}

#[allow(unsafe_code)]
fn restore_terminal() {
    if let Some(saved) = SAVED_TERMIOS.get() {
        // SAFETY: `saved` came from tcgetattr on the same descriptor.
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, saved) };
    }
}

/// Delivers keystrokes, including Ctrl-C, straight to the monitor.
#[allow(unsafe_code)]
fn make_terminal_raw() -> bool {
    // SAFETY: plain libc calls on standard input with a zeroed termios.
    unsafe {
        if libc::isatty(libc::STDIN_FILENO) == 0 {
            return false;
        }
        let mut saved: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(libc::STDIN_FILENO, &raw mut saved) < 0 {
            return false;
        }
        let _ = SAVED_TERMIOS.set(saved);
        let mut raw = saved;
        raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
        raw.c_iflag &= !(libc::IXON | libc::ICRNL);
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &raw const raw);
    }
    eprintln!("sim6502: Ctrl-] quits");
    true
}

/// Serves the monitor on a new pseudo-terminal instead of this terminal.
#[allow(unsafe_code)]
fn open_virtual_port() {
    // SAFETY: plain libc calls; every result is checked before use, and
    // the name from ptsname is copied before any other pty call.
    unsafe {
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        if master < 0 || libc::grantpt(master) < 0 || libc::unlockpt(master) < 0 {
            eprintln!("sim6502: pseudo-terminal: {}", io::Error::last_os_error());
            process::exit(1);
        }
        let name_ptr = libc::ptsname(master);
        if name_ptr.is_null() {
            eprintln!("sim6502: pseudo-terminal: {}", io::Error::last_os_error());
            process::exit(1);
        }
        let name = CStr::from_ptr(name_ptr).to_owned();

        // Holding the port's far end open keeps it usable between client
        // connections; raw mode passes binary uploads through untouched.
        let port = libc::open(name.as_ptr(), libc::O_RDWR | libc::O_NOCTTY);
        let mut settings: libc::termios = std::mem::zeroed();
        if port < 0 || libc::tcgetattr(port, &raw mut settings) < 0 {
            eprintln!("{}: {}", name.to_string_lossy(), io::Error::last_os_error());
            process::exit(1);
        }
        libc::cfmakeraw(&raw mut settings);
        libc::tcsetattr(port, libc::TCSANOW, &raw const settings);

        eprintln!(
            "sim6502: serial port is {} (Ctrl-C quits)",
            name.to_string_lossy()
        );
        libc::dup2(master, libc::STDIN_FILENO);
        libc::dup2(master, libc::STDOUT_FILENO);
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: sim6502 [-p] [FILE.bin [LOAD_ADDR]]\n  \
         Loads FILE (default: the built-in demo) at LOAD_ADDR (hex, default 0200)\n  \
         and runs it.\n  \
         -p  serve the monitor on a virtual serial port instead of this terminal"
    );
    process::exit(2);
}

fn main() {
    let mut virtual_port = false;
    let mut files = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-p" => virtual_port = true,
            _ if arg.starts_with('-') => usage(),
            _ => files.push(arg),
        }
    }
    if files.len() > 2 {
        usage();
    }

    let mut machine = Box::new(Machine::new(NoDisplay));
    machine.init();

    let mut start = DEMO_LOAD_ADDR;
    if let Some(path) = files.first() {
        start = match files.get(1) {
            Some(text) => {
                u16::from_str_radix(text.trim_start_matches('$'), 16).unwrap_or_else(|_| usage())
            }
            None => 0x0200,
        };
        let data = std::fs::read(path).unwrap_or_else(|error| {
            eprintln!("{path}: {error}");
            process::exit(1);
        });
        let ram = machine.bus.ram_mut();
        let n = data.len().min(ram.len() - usize::from(start));
        ram[usize::from(start)..usize::from(start) + n].copy_from_slice(&data[..n]);
        eprintln!("sim6502: loaded {n} bytes at ${start:04X}");
    } else {
        machine
            .bus
            .load(DEMO_LOAD_ADDR, DEMO_PROGRAM)
            .expect("the demo fits below the I/O page");
    }

    let raw = if virtual_port {
        open_virtual_port();
        false
    } else {
        make_terminal_raw()
    };

    let mut terminal = Terminal {
        out: BufWriter::new(io::stdout()),
        start: Instant::now(),
        raw,
        hold_input: !raw && !virtual_port,
        input_closed: false,
    };
    let mut monitor = Monitor::new();
    block_on(async {
        monitor.init(&machine, &mut terminal).await;
        monitor.go(&mut machine, &mut terminal, start).await;
        while terminal.hold_input && monitor.is_running() && terminal.time_us() < 1_000_000 {
            monitor.poll(&mut machine, &mut terminal).await;
        }
        terminal.hold_input = false;
        while !terminal.input_closed || monitor.is_running() {
            monitor.poll(&mut machine, &mut terminal).await;
        }
        terminal.flush().await;
    });
    quit(0);
}

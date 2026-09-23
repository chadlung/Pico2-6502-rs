//! Pico 2 firmware: USB serial monitor, 6502 CPU and 16x2 LCD.
//!
//! The monitor runs on USB CDC.  Like the Pico SDK's USB serial, the device
//! reboots into BOOTSEL when a host sets 1200 baud, and has the reset
//! interface `picotool -f` uses.  A timestamped debug log goes to UART0 TX
//! on GP0 at 115200 8N1, for a Raspberry Pi Debug Probe.

#![no_std]
#![no_main]

use core::fmt::{self, Write as _};
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU8, Ordering};

use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_futures::yield_now;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{self, I2c};
use embassy_rp::pac;
use embassy_rp::peripherals::{I2C0, PIN_0, UART0, USB};
use embassy_rp::rom_data::reset_to_usb_boot;
use embassy_rp::uart::{self, Blocking, UartTx};
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_time::{Duration, Instant, Timer, block_for};
use embassy_usb::class::cdc_acm::{CdcAcmClass, ControlChanged, Receiver, Sender, State};
use embassy_usb::control::{OutResponse, Recipient, Request, RequestType};
use embassy_usb::driver::EndpointError;
use embassy_usb::types::InterfaceNumber;
use embassy_usb::{Handler, UsbDevice};
use heapless::Deque;
use pico2_6502_rs::lcd::LcdHardware;
use pico2_6502_rs::machine::Machine;
use pico2_6502_rs::monitor::Monitor;
use pico2_6502_rs::platform::Platform;
use pico2_6502_rs::{DEMO_LOAD_ADDR, DEMO_PROGRAM};
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

type UsbDriver = Driver<'static, USB>;

/// Writes one timestamped line to the UART debug log.
macro_rules! debug_log {
    ($log:expr, $($arg:tt)*) => {
        $log.line(format_args!($($arg)*))
    };
}

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, UsbDriver>) -> ! {
    usb.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(embassy_rp::config::Config::default());
    let mut log = DebugLog(UartTx::new_blocking(
        p.UART0,
        p.PIN_0,
        uart::Config::default(),
    ));
    debug_log!(
        log,
        "Pico 6502 (Rust) v{} booting",
        env!("CARGO_PKG_VERSION")
    );
    let led = Output::new(p.PIN_25, Level::Low);

    // The module has its own pull-up resistors, so the Pico's are left off.
    let mut i2c_config = i2c::Config::default();
    i2c_config.frequency = LCD_I2C_BAUD;
    i2c_config.sda_pullup = false;
    i2c_config.scl_pullup = false;
    let i2c = I2c::new_blocking(p.I2C0, p.PIN_5, p.PIN_4, i2c_config);

    let driver = Driver::new(p.USB, Irqs);
    let mut usb_config = embassy_usb::Config::new(0x2E8A, 0x0009);
    usb_config.manufacturer = Some("Raspberry Pi");
    usb_config.product = Some("Pico 6502 (Rust)");
    // The chip ID, as the boot ROM and the Pico SDK use, lets `picotool -f`
    // find the Pico again once it has rebooted into BOOTSEL.
    static SERIAL_NUMBER: StaticCell<[u8; 16]> = StaticCell::new();
    usb_config.serial_number = Some(chip_id_hex(SERIAL_NUMBER.init([b'0'; 16])));
    usb_config.max_power = 100;
    usb_config.max_packet_size_0 = 64;

    static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUFFER: StaticCell<[u8; 64]> = StaticCell::new();
    static CDC_STATE: StaticCell<State> = StaticCell::new();
    static RESET_HANDLER: StaticCell<ResetInterface> = StaticCell::new();

    let mut builder = embassy_usb::Builder::new(
        driver,
        usb_config,
        CONFIG_DESCRIPTOR.init([0; 256]),
        BOS_DESCRIPTOR.init([0; 256]),
        &mut [],
        CONTROL_BUFFER.init([0; 64]),
    );
    let serial = CdcAcmClass::new(&mut builder, CDC_STATE.init(State::new()), 64);
    let reset_interface = {
        let mut function = builder.function(0xFF, RESET_SUBCLASS, RESET_PROTOCOL);
        let mut interface = function.interface();
        let number = interface.interface_number();
        interface.alt_setting(0xFF, RESET_SUBCLASS, RESET_PROTOCOL, None);
        number
    };
    builder.handler(RESET_HANDLER.init(ResetInterface(reset_interface)));
    let (sender, receiver, control) = serial.split_with_control();
    spawner.spawn(usb_task(builder.build()).expect("USB task token"));

    let mut machine = Machine::new(Pcf8574Lcd::new(i2c));
    machine.init();
    match machine.bus.lcd().address() {
        Some(address) => debug_log!(log, "LCD: PCF8574 backpack at I2C 0x{address:02X}"),
        None => debug_log!(log, "LCD: no PCF8574 backpack found on I2C0 (GP4/GP5)"),
    }

    // Start the built-in demo so the display shows signs of life right away.
    machine
        .bus
        .load(DEMO_LOAD_ADDR, DEMO_PROGRAM)
        .expect("the demo fits below the I/O page");
    let mut terminal = UsbTerminal::new(sender, receiver, control, led, log);
    let mut monitor = Monitor::new();
    monitor.init(&machine, &mut terminal).await;
    monitor
        .go(&mut machine, &mut terminal, DEMO_LOAD_ADDR)
        .await;

    let mut was_running = false;
    let mut shown_lcd = false;
    loop {
        let running = monitor.is_running();
        if running != was_running {
            was_running = running;
            log_run_state(&mut terminal.log, &monitor, &machine);
            if !running && !shown_lcd {
                shown_lcd = true;
                for row in machine.bus.lcd().text() {
                    let text = core::str::from_utf8(row).unwrap_or("?");
                    debug_log!(terminal.log, "LCD: |{text}|");
                }
            }
        }
        monitor.poll(&mut machine, &mut terminal).await;
    }
}

/// Writes the chip ID as 16 upper-case hex digits.
fn chip_id_hex(text: &'static mut [u8; 16]) -> &'static str {
    let id = embassy_rp::otp::get_chipid().unwrap_or(0);
    for (i, c) in text.iter_mut().enumerate() {
        let digit = (id >> (60 - 4 * i)) & 0xF;
        *c = b"0123456789ABCDEF"[digit as usize];
    }
    core::str::from_utf8(text).unwrap_or("0")
}

/// Logs a 6502 start or stop.
fn log_run_state(log: &mut DebugLog, monitor: &Monitor, machine: &Machine<Pcf8574Lcd>) {
    if monitor.is_running() {
        debug_log!(log, "6502: running from ${:04X}", monitor.run_start_pc());
    } else if let Some(reason) = monitor.last_stop() {
        debug_log!(log, "6502: stopped at ${:04X} ({reason:?})", machine.cpu.pc);
    }
}

// ------------------ USB serial terminal -------------------------------------

/// The monitor's terminal on USB CDC.
///
/// Output is held (up to 8 KiB, then dropped) until a terminal opens the
/// port by asserting DTR, so nothing printed before then is lost.  While the
/// port is open, output waits for the host, which paces the 6502 as the Pico
/// SDK's blocking `printf` does.
struct UsbTerminal {
    sender: Sender<'static, UsbDriver>,
    receiver: Receiver<'static, UsbDriver>,
    control: ControlChanged<'static>,
    received: [u8; 64],
    received_len: usize,
    received_pos: usize,
    output: Deque<u8, 8192>,
    packet: [u8; 64],
    packet_len: usize,
    /// The last packet was full, so the host needs a short one to see the
    /// end of the transfer.
    needs_short_packet: bool,
    open: bool,
    led: Output<'static>,
    log: DebugLog,
}

impl UsbTerminal {
    fn new(
        sender: Sender<'static, UsbDriver>,
        receiver: Receiver<'static, UsbDriver>,
        control: ControlChanged<'static>,
        led: Output<'static>,
        log: DebugLog,
    ) -> Self {
        Self {
            sender,
            receiver,
            control,
            received: [0; 64],
            received_len: 0,
            received_pos: 0,
            output: Deque::new(),
            packet: [0; 64],
            packet_len: 0,
            needs_short_packet: false,
            open: false,
            led,
            log,
        }
    }

    /// Follows the port's control lines: DTR, a 1200 baud request for
    /// BOOTSEL, and requests on the reset interface.
    async fn service(&mut self) {
        let dtr = self.control.dtr();
        if dtr != self.open {
            self.open = dtr;
            if dtr {
                debug_log!(self.log, "USB: terminal opened (DTR on)");
            } else {
                debug_log!(self.log, "USB: terminal closed (DTR off)");
                self.needs_short_packet = false;
            }
        }
        if self.receiver.line_coding().data_rate() == MAGIC_BAUD_RATE {
            debug_log!(self.log, "USB: 1200 baud, rebooting into BOOTSEL");
            reboot(RESET_REQUEST_BOOTSEL).await;
        }
        match RESET_REQUEST.load(Ordering::Relaxed) {
            0 => {}
            request => {
                debug_log!(self.log, "USB: picotool reset request {request}");
                reboot(request).await;
            }
        }
    }

    /// Sends held output while the port is open.
    async fn send(&mut self) {
        if !self.open {
            return;
        }
        loop {
            while self.packet_len < self.packet.len() {
                let Some(byte) = self.output.pop_front() else {
                    break;
                };
                self.packet[self.packet_len] = byte;
                self.packet_len += 1;
            }
            if self.packet_len == 0 && !self.needs_short_packet {
                return;
            }
            let packet = &self.packet[..self.packet_len];
            match select(self.sender.write_packet(packet), closed(&self.control)).await {
                Either::First(Ok(())) => {}
                // Keep the packet for the next time the port opens.
                Either::First(Err(_)) | Either::Second(()) => return,
            }
            self.needs_short_packet = self.packet_len == self.packet.len();
            self.packet_len = 0;
        }
    }
}

impl Platform for UsbTerminal {
    async fn getc(&mut self, timeout_us: u32) -> Option<u8> {
        self.service().await;
        self.send().await;
        if self.received_pos < self.received_len {
            let byte = self.received[self.received_pos];
            self.received_pos += 1;
            return Some(byte);
        }
        let read = self.receiver.read_packet(&mut self.received);
        let result = if timeout_us == 0 {
            // Let the USB task run even when the 6502 never waits.
            match select(read, yield_now()).await {
                Either::First(result) => Some(result),
                Either::Second(()) => None,
            }
        } else {
            match select(read, Timer::after_micros(u64::from(timeout_us))).await {
                Either::First(result) => Some(result),
                Either::Second(()) => None,
            }
        };
        match result {
            Some(Ok(len)) if len > 0 => {
                self.received_len = len;
                self.received_pos = 1;
                Some(self.received[0])
            }
            Some(Err(EndpointError::Disabled)) => {
                Timer::after_micros(u64::from(timeout_us)).await;
                None
            }
            _ => None,
        }
    }

    async fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.output.is_full() {
                self.send().await;
            }
            // Dropped if the port is closed and the buffer is full.
            let _ = self.output.push_back(byte);
        }
    }

    async fn flush(&mut self) {
        self.send().await;
    }

    fn time_us(&mut self) -> u64 {
        Instant::now().as_micros()
    }

    async fn sleep_us(&mut self, us: u64) {
        self.send().await;
        Timer::after_micros(us).await;
    }

    fn led(&mut self, on: bool) {
        self.led
            .set_level(if on { Level::High } else { Level::Low });
    }
}

/// Completes once the host drops DTR.
async fn closed(control: &ControlChanged<'_>) {
    while control.dtr() {
        control.control_changed().await;
    }
}

// ------------------ Rebooting ------------------------------------------------

/// Setting this baud rate reboots into BOOTSEL, as with the Pico SDK.
const MAGIC_BAUD_RATE: u32 = 1200;

// picotool's reset interface: vendor class, this subclass and protocol.
const RESET_SUBCLASS: u8 = 0x00;
const RESET_PROTOCOL: u8 = 0x01;
const RESET_REQUEST_BOOTSEL: u8 = 0x01;
const RESET_REQUEST_FLASH: u8 = 0x02;

/// A reboot requested on the reset interface, handled by the main task.
static RESET_REQUEST: AtomicU8 = AtomicU8::new(0);

/// The reset interface of the Pico SDK's USB serial, so `picotool -f` can
/// reboot the Pico into BOOTSEL (or back into this firmware).
struct ResetInterface(InterfaceNumber);

impl Handler for ResetInterface {
    fn control_out(&mut self, req: Request, _data: &[u8]) -> Option<OutResponse> {
        if req.request_type != RequestType::Class
            || req.recipient != Recipient::Interface
            || req.index != u16::from(self.0.0)
        {
            return None;
        }
        match req.request {
            RESET_REQUEST_BOOTSEL | RESET_REQUEST_FLASH => {
                RESET_REQUEST.store(req.request, Ordering::Relaxed);
                Some(OutResponse::Accepted)
            }
            _ => Some(OutResponse::Rejected),
        }
    }
}

/// Reboots into BOOTSEL or into this firmware, after giving USB time to
/// finish the request.
async fn reboot(request: u8) -> ! {
    Timer::after_millis(20).await;
    if request == RESET_REQUEST_BOOTSEL {
        reset_to_usb_boot(0, 0);
    }
    cortex_m::peripheral::SCB::sys_reset();
}

// ------------------ PCF8574 LCD backpack ------------------------------------

// HD44780 16x2 LCD on a PCF8574 I2C backpack, as on the Freenove I2C LCD1602
// module.  PCF8574T boards answer at 0x27 and PCF8574AT boards at 0x3F;
// soldering the A0-A2 pads moves them within 0x20-0x27 or 0x38-0x3F, so all
// of those addresses are tried.  Bit assignments follow Freenove's
// I2C_LCD.py and the LiquidCrystal_I2C library.

const LCD_SDA_PIN: usize = 4; // GP4, physical pin 6
const LCD_SCL_PIN: usize = 5; // GP5, physical pin 7
const LCD_I2C_BAUD: u32 = 100_000; // the PCF8574 is a 100 kHz part

// PCF8574 outputs P0-P7 on the backpack.
const BIT_RS: u8 = 0x01;
const BIT_E: u8 = 0x04; // P1 (RW) stays low: the display is only written
const BIT_BACKLIGHT: u8 = 0x08;
const DATA_SHIFT: u8 = 4; // P4-P7 = D4-D7

struct Pcf8574Lcd {
    i2c: I2c<'static, I2C0, i2c::Blocking>,
    /// `None` until a backpack answers.
    address: Option<u8>,
    /// The PCF8574 has one register: its output pins.
    outputs: u8,
}

impl Pcf8574Lcd {
    const fn new(i2c: I2c<'static, I2C0, i2c::Blocking>) -> Self {
        Self {
            i2c,
            address: None,
            outputs: 0,
        }
    }

    fn set_outputs(&mut self, value: u8) {
        self.outputs = value;
        if let Some(address) = self.address {
            let _ = self.i2c.blocking_write(address, &[value]);
        }
    }

    /// Puts a nibble on D4-D7 and pulses E; the display latches on the
    /// falling edge.  Each I2C transaction takes far longer than the
    /// HD44780's minimum timings.
    fn write_nibble(&mut self, nibble: u8) {
        let out = (self.outputs & (BIT_RS | BIT_BACKLIGHT)) | ((nibble & 0x0F) << DATA_SHIFT);
        self.set_outputs(out);
        self.set_outputs(out | BIT_E);
        self.set_outputs(out);
    }

    fn write_byte(&mut self, value: u8, rs: bool) {
        if rs {
            self.outputs |= BIT_RS;
        } else {
            self.outputs &= !BIT_RS;
        }
        self.write_nibble(value >> 4);
        self.write_nibble(value);
    }

    fn find_backpack(&mut self) -> Option<u8> {
        const CANDIDATES: [u8; 16] = [
            0x27, 0x3F, // factory addresses
            0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, // PCF8574T, address pads soldered
            0x38, 0x39, 0x3A, 0x3B, 0x3C, 0x3D, 0x3E, // PCF8574AT, address pads soldered
        ];
        CANDIDATES
            .into_iter()
            .find(|&address| self.i2c.blocking_write(address, &[0]).is_ok())
    }
}

/// Whether a GPIO pin reads high.
fn pin_high(pin: usize) -> bool {
    pac::SIO.gpio_in(0).read() & (1 << pin) != 0
}

/// Whether both I2C lines are high, as on an idle bus.  A line held low
/// could stall the I2C controller, which has no timeout, so the bus is only
/// used when this is true.
fn bus_idle() -> bool {
    pin_high(LCD_SDA_PIN) && pin_high(LCD_SCL_PIN)
}

/// An idle I2C line is held high by the module's pull-up resistors.  For a
/// line that reads low, switching on the Pico's weak pull-up tells two
/// faults apart.
fn report_line(out: &mut dyn fmt::Write, name: &str, pin: usize) {
    if pin_high(pin) {
        let _ = writeln!(out, "{name} (GP{pin}) is high, as it should be");
        return;
    }
    let pad = pac::PADS_BANK0.gpio(pin);
    let saved = pad.read();
    pad.modify(|w| {
        w.set_pue(true);
        w.set_pde(false);
    });
    block_for(Duration::from_millis(5));
    let rises = pin_high(pin);
    pad.write_value(saved);
    let reads = if rises {
        "high: the pin is not connected to the module,\n  or the module has no pull-up resistors"
    } else {
        "LOW: something holds it down, usually a module\n  without power (check VCC and GND) or a short to ground"
    };
    let _ = writeln!(
        out,
        "{name} (GP{pin}) is LOW. With the Pico's pull-up it reads {reads}"
    );
}

impl LcdHardware for Pcf8574Lcd {
    fn init(&mut self) -> bool {
        block_for(Duration::from_millis(50)); // HD44780 needs >40 ms after power-up

        self.address = None;
        if !bus_idle() {
            return false;
        }
        self.address = self.find_backpack();
        if self.address.is_none() {
            return false;
        }
        self.set_outputs(0);

        // 4-bit initialisation sequence (HD44780 datasheet, figure 24).
        self.write_nibble(0x03);
        block_for(Duration::from_micros(4500));
        self.write_nibble(0x03);
        block_for(Duration::from_micros(4500));
        self.write_nibble(0x03);
        block_for(Duration::from_micros(150));
        self.write_nibble(0x02);

        self.command(0x28); // function set: 4-bit, 2 lines, 5x8 font
        self.command(0x0C); // display on, cursor off, blink off
        self.command(0x06); // entry mode: cursor moves right, no shift
        self.command(0x01); // clear
        true
    }

    fn diagnose(&mut self, out: &mut dyn fmt::Write) {
        report_line(out, "SDA", LCD_SDA_PIN);
        report_line(out, "SCL", LCD_SCL_PIN);
        let _ = out.write_str("I2C devices answering:");
        let mut found = 0;
        // Nothing can answer on a bus with a line held low.
        if bus_idle() {
            for address in 0x08u8..0x78 {
                if self.i2c.blocking_read(address, &mut [0]).is_ok() {
                    let _ = write!(out, " {address:02X}");
                    found += 1;
                }
            }
        }
        let _ = out.write_str(if found == 0 { " none\n" } else { "\n" });
    }

    fn address(&self) -> Option<u8> {
        self.address
    }

    fn command(&mut self, command: u8) {
        self.write_byte(command, false);
        if command <= 0x03 {
            block_for(Duration::from_millis(2)); // clear and home take up to 1.52 ms
        }
    }

    fn write(&mut self, data: u8) {
        self.write_byte(data, true);
    }

    fn backlight(&mut self, on: bool) {
        let value = if on {
            self.outputs | BIT_BACKLIGHT
        } else {
            self.outputs & !BIT_BACKLIGHT
        };
        self.set_outputs(value);
    }
}

// ------------------ UART debug log ------------------------------------------

/// Timestamped debug log on UART0 TX (GP0), 115200 8N1.
struct DebugLog(UartTx<'static, Blocking>);

impl DebugLog {
    fn line(&mut self, args: fmt::Arguments<'_>) {
        let ms = Instant::now().as_millis();
        let _ = write!(self, "[{:5}.{:03}] {args}\r\n", ms / 1000, ms % 1000);
    }
}

impl fmt::Write for DebugLog {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0
            .blocking_write(text.as_bytes())
            .map_err(|_| fmt::Error)
    }
}

#[panic_handler]
#[allow(unsafe_code)]
fn panic(info: &PanicInfo<'_>) -> ! {
    cortex_m::interrupt::disable();
    // SAFETY: interrupts are off and the executor never runs again, so this
    // is the only remaining user of UART0 and GP0.
    let (uart0, pin0) = unsafe { (UART0::steal(), PIN_0::steal()) };
    let mut log = DebugLog(UartTx::new_blocking(uart0, pin0, uart::Config::default()));
    debug_log!(log, "PANIC: {info}");
    let _ = log.0.blocking_flush();
    loop {
        cortex_m::asm::wfi();
    }
}

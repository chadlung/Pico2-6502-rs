//! Raspberry Pi Pico 2 firmware front end.

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{self, I2c};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_time::{Duration, Timer, block_for};
use embassy_usb::UsbDevice;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::EndpointError;
use panic_halt as _;
use pico2_6502_rs::bus::{LcdEvent, MachineBus};
use pico2_6502_rs::machine::Machine;
use pico2_6502_rs::monitor::Monitor;
use pico2_6502_rs::{DEMO_LOAD_ADDR, DEMO_PROGRAM};
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

type UsbDriver = Driver<'static, USB>;
type PicoUsbDevice = UsbDevice<'static, UsbDriver>;

#[embassy_executor::task]
async fn usb_task(mut usb: PicoUsbDevice) -> ! {
    usb.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let peripherals = embassy_rp::init(embassy_rp::config::Config::default());
    let mut led = Output::new(peripherals.PIN_25, Level::Low);

    let i2c = I2c::new_blocking(
        peripherals.I2C0,
        peripherals.PIN_5,
        peripherals.PIN_4,
        i2c::Config::default(),
    );
    let mut display = Pcf8574Lcd::new(i2c);

    let driver = Driver::new(peripherals.USB, Irqs);
    let mut usb_config = embassy_usb::Config::new(0x2E8A, 0x0009);
    usb_config.manufacturer = Some("Raspberry Pi");
    usb_config.product = Some("Pico 6502 (Rust)");
    usb_config.serial_number = Some("PICO2-6502-RS");
    usb_config.max_power = 100;
    usb_config.max_packet_size_0 = 64;

    static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUFFER: StaticCell<[u8; 64]> = StaticCell::new();
    static CDC_STATE: StaticCell<State> = StaticCell::new();

    let mut builder = embassy_usb::Builder::new(
        driver,
        usb_config,
        CONFIG_DESCRIPTOR.init([0; 256]),
        BOS_DESCRIPTOR.init([0; 256]),
        &mut [],
        CONTROL_BUFFER.init([0; 64]),
    );
    let mut serial = CdcAcmClass::new(&mut builder, CDC_STATE.init(State::new()), 64);
    let usb = builder.build();
    spawner.spawn(usb_task(usb).expect("USB task token"));

    let mut machine = Machine::new();
    machine.cpu.memory.set_display_address(display.address());
    machine
        .cpu
        .memory
        .load(DEMO_LOAD_ADDR, DEMO_PROGRAM)
        .expect("built-in demo fits in RAM");
    let mut monitor = Monitor::new();
    monitor.init(&machine);
    monitor.start_at(&mut machine, DEMO_LOAD_ADDR);

    // Run the short greeting before a terminal connects so the LCD is useful
    // immediately after boot.
    while monitor.is_running() {
        led.set_high();
        let _ = monitor.tick(&mut machine);
        apply_lcd_events(&mut machine.cpu.memory, &mut display);
    }
    led.set_low();

    loop {
        serial.wait_connection().await;
        let mut receive = [0u8; 64];
        loop {
            match select(serial.read_packet(&mut receive), Timer::after_millis(1)).await {
                Either::First(Ok(count)) => {
                    for &byte in &receive[..count] {
                        monitor.input(&mut machine, byte);
                    }
                }
                Either::First(Err(EndpointError::Disabled)) => break,
                Either::First(Err(EndpointError::BufferOverflow)) | Either::Second(()) => {}
            }

            if monitor.is_running() {
                led.set_high();
                let target = if monitor.speed_khz() == 0 {
                    u64::MAX
                } else {
                    u64::from(monitor.speed_khz())
                };
                let mut cycles = 0;
                let mut instructions = 0;
                while monitor.is_running() && cycles < target && instructions < 20_000 {
                    cycles += monitor.tick(&mut machine);
                    instructions += 1;
                    apply_lcd_events(&mut machine.cpu.memory, &mut display);
                }
            } else {
                led.set_low();
            }

            let mut packet = [0u8; 64];
            let mut count = 0;
            while count < packet.len() {
                let byte = machine
                    .cpu
                    .memory
                    .pop_serial_tx()
                    .or_else(|| monitor.pop_output());
                let Some(byte) = byte else { break };
                packet[count] = byte;
                count += 1;
            }
            if count != 0 && serial.write_packet(&packet[..count]).await.is_err() {
                break;
            }
        }
    }
}

fn apply_lcd_events<I>(bus: &mut MachineBus, display: &mut Pcf8574Lcd<I>)
where
    I: embedded_hal::i2c::I2c,
{
    while let Some(event) = bus.pop_lcd_event() {
        display.apply(event);
    }
}

struct Pcf8574Lcd<I> {
    i2c: I,
    address: Option<u8>,
    outputs: u8,
}

impl<I> Pcf8574Lcd<I>
where
    I: embedded_hal::i2c::I2c,
{
    const BIT_RS: u8 = 0x01;
    const BIT_E: u8 = 0x04;
    const BIT_BACKLIGHT: u8 = 0x08;

    fn new(mut i2c: I) -> Self {
        const CANDIDATES: [u8; 16] = [
            0x27, 0x3F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x38, 0x39, 0x3A, 0x3B, 0x3C,
            0x3D, 0x3E,
        ];
        block_for(Duration::from_millis(50));
        let address = CANDIDATES
            .iter()
            .copied()
            .find(|address| i2c.write(*address, &[0]).is_ok());
        let mut lcd = Self {
            i2c,
            address,
            outputs: 0,
        };
        if lcd.address.is_some() {
            lcd.write_nibble(0x03);
            block_for(Duration::from_micros(4500));
            lcd.write_nibble(0x03);
            block_for(Duration::from_micros(4500));
            lcd.write_nibble(0x03);
            block_for(Duration::from_micros(150));
            lcd.write_nibble(0x02);
            lcd.command(0x28);
            lcd.command(0x0C);
            lcd.command(0x06);
            lcd.command(0x01);
        }
        lcd
    }

    const fn address(&self) -> Option<u8> {
        self.address
    }

    fn apply(&mut self, event: LcdEvent) {
        match event {
            LcdEvent::Clear => self.command(0x01),
            LcdEvent::Home => self.command(0x02),
            LcdEvent::Position { row, col } => {
                let base = if row == 0 { 0x00 } else { 0x40 };
                self.command(0x80 | (base + col));
            }
            LcdEvent::Write(byte) => self.write_byte(byte, true),
            LcdEvent::Backlight(on) => {
                if on {
                    self.outputs |= Self::BIT_BACKLIGHT;
                } else {
                    self.outputs &= !Self::BIT_BACKLIGHT;
                }
                self.flush();
            }
        }
    }

    fn command(&mut self, command: u8) {
        self.write_byte(command, false);
        if command <= 0x03 {
            block_for(Duration::from_millis(2));
        }
    }

    fn write_byte(&mut self, byte: u8, data: bool) {
        if data {
            self.outputs |= Self::BIT_RS;
        } else {
            self.outputs &= !Self::BIT_RS;
        }
        self.write_nibble(byte >> 4);
        self.write_nibble(byte);
    }

    fn write_nibble(&mut self, nibble: u8) {
        let value = (self.outputs & (Self::BIT_RS | Self::BIT_BACKLIGHT)) | ((nibble & 0x0F) << 4);
        self.outputs = value;
        self.flush();
        self.outputs = value | Self::BIT_E;
        self.flush();
        self.outputs = value;
        self.flush();
    }

    fn flush(&mut self) {
        if let Some(address) = self.address {
            let _ = self.i2c.write(address, &[self.outputs]);
        }
    }
}

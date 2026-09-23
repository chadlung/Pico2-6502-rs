//! Runs the 6502 CPU test suites from the Fake6502 repository against this
//! project's CPU core, with the same programs and pass criteria as that
//! repository's `test.c`.  `make test` in `host/` downloads them and runs this.
//!
//! Tests of the unstable undocumented opcodes (ANE, LXA, SHA, SHX, SHY and
//! TAS) are listed but skipped, because the core leaves those opcodes out:
//! seven Lorenz tests, and Avery Lee's `avery3`, which also checks SHX and SHY.
//!
//! usage: `cputest [TESTS_DIR]` (default `host/fake6502-upstream`)

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pico2_6502_rs::cpu::{Cpu, Memory};

const PASS: &str = "\x1b[1;32mpass\x1b[0m";
const FAIL: &str = "\x1b[1;31mFAIL!\x1b[0m";
const SKIP: &str = "\x1b[1;33mskip\x1b[0m";

/// A flat 64K of RAM: the test programs expect no I/O page.
struct Ram(Vec<u8>);

impl Memory for Ram {
    fn read(&mut self, address: u16) -> u8 {
        self.0[usize::from(address)]
    }

    fn write(&mut self, address: u16, value: u8) {
        self.0[usize::from(address)] = value;
    }
}

enum Test {
    Suite(&'static str),
    Run(&'static str, u16),
    /// A test that needs opcodes the core deliberately leaves out.
    Skip(&'static str, &'static str),
}

const UNSTABLE: &str = "unstable opcode, not emulated";

const TESTS: &[Test] = &[
    Test::Suite("Klaus Dormann test suite."),
    Test::Run("tests/6502_functional_test.bin", 0x3469),
    Test::Run("tests/6502_decimal_test.bin", 0x044B),
    Test::Suite("Bird Computer test suite."),
    Test::Run("tests/bird6502.bin", 0x861C),
    Test::Suite("Ruud Baltissen test suite."),
    Test::Run("tests/ttl6502.bin", 0xF5EA),
    Test::Suite("Lorenz test suite for undocumented opcodes."),
    Test::Run("tests/lorenz/slo_asoa.bin", 0x08B3),
    Test::Run("tests/lorenz/slo_asoax.bin", 0x08CA),
    Test::Run("tests/lorenz/slo_asoay.bin", 0x08CA),
    Test::Run("tests/lorenz/slo_asoix.bin", 0x08C4),
    Test::Run("tests/lorenz/slo_asoiy.bin", 0x08CE),
    Test::Run("tests/lorenz/slo_asoz.bin", 0x08B6),
    Test::Run("tests/lorenz/slo_asozx.bin", 0x08C0),
    Test::Run("tests/lorenz/rlaa.bin", 0x08AA),
    Test::Run("tests/lorenz/rlaax.bin", 0x08C0),
    Test::Run("tests/lorenz/rlaay.bin", 0x08C0),
    Test::Run("tests/lorenz/rlaix.bin", 0x08BA),
    Test::Run("tests/lorenz/rlaiy.bin", 0x08C4),
    Test::Run("tests/lorenz/rlaz.bin", 0x08AD),
    Test::Run("tests/lorenz/rlazx.bin", 0x08B6),
    Test::Run("tests/lorenz/sre_lsea.bin", 0x08A8),
    Test::Run("tests/lorenz/sre_lseax.bin", 0x08BE),
    Test::Run("tests/lorenz/sre_lseay.bin", 0x08BE),
    Test::Run("tests/lorenz/sre_lseix.bin", 0x08B8),
    Test::Run("tests/lorenz/sre_lseiy.bin", 0x08C2),
    Test::Run("tests/lorenz/sre_lsez.bin", 0x08AB),
    Test::Run("tests/lorenz/sre_lsezx.bin", 0x08B4),
    Test::Run("tests/lorenz/rraa.bin", 0x0887),
    Test::Run("tests/lorenz/rraax.bin", 0x089D),
    Test::Run("tests/lorenz/rraay.bin", 0x089D),
    Test::Run("tests/lorenz/rraix.bin", 0x0897),
    Test::Run("tests/lorenz/rraiy.bin", 0x08A1),
    Test::Run("tests/lorenz/rraz.bin", 0x088A),
    Test::Run("tests/lorenz/rrazx.bin", 0x0893),
    Test::Run("tests/lorenz/sax_axsa.bin", 0x088D),
    Test::Run("tests/lorenz/sax_axsix.bin", 0x0897),
    Test::Run("tests/lorenz/sax_axsz.bin", 0x0890),
    Test::Run("tests/lorenz/sax_axszy.bin", 0x0899),
    Test::Run("tests/lorenz/laxa.bin", 0x088E),
    Test::Run("tests/lorenz/laxay.bin", 0x08A4),
    Test::Run("tests/lorenz/laxix.bin", 0x089E),
    Test::Run("tests/lorenz/laxiy.bin", 0x08A8),
    Test::Run("tests/lorenz/laxz.bin", 0x0891),
    Test::Run("tests/lorenz/laxzy.bin", 0x089A),
    Test::Run("tests/lorenz/dcp_dcma.bin", 0x088C),
    Test::Run("tests/lorenz/dcp_dcmax.bin", 0x08A2),
    Test::Run("tests/lorenz/dcp_dcmay.bin", 0x08A2),
    Test::Run("tests/lorenz/dcp_dcmix.bin", 0x089C),
    Test::Run("tests/lorenz/dcp_dcmiy.bin", 0x08A6),
    Test::Run("tests/lorenz/dcp_dcmz.bin", 0x088F),
    Test::Run("tests/lorenz/dcp_dcmzx.bin", 0x0898),
    Test::Run("tests/lorenz/isc_insa.bin", 0x088C),
    Test::Run("tests/lorenz/isc_insax.bin", 0x08A2),
    Test::Run("tests/lorenz/isc_insay.bin", 0x08A2),
    Test::Run("tests/lorenz/isc_insix.bin", 0x089C),
    Test::Run("tests/lorenz/isc_insiy.bin", 0x08A6),
    Test::Run("tests/lorenz/isc_insz.bin", 0x088F),
    Test::Run("tests/lorenz/isc_inszx.bin", 0x0898),
    Test::Run("tests/lorenz/ancb.bin", 0x08D8),
    Test::Run("tests/lorenz/alrb.bin", 0x08AA),
    Test::Run("tests/lorenz/arrb.bin", 0x0947),
    Test::Run("tests/lorenz/sbxb.bin", 0x08C3),
    Test::Run("tests/lorenz/lasay.bin", 0x08F1),
    Test::Skip("tests/lorenz/shaay.bin", UNSTABLE),
    Test::Skip("tests/lorenz/shaiy.bin", UNSTABLE),
    Test::Skip("tests/lorenz/shxay.bin", UNSTABLE),
    Test::Skip("tests/lorenz/shyax.bin", UNSTABLE),
    Test::Skip("tests/lorenz/tas_shsay.bin", UNSTABLE),
    Test::Skip("tests/lorenz/aneb.bin", UNSTABLE),
    Test::Skip("tests/lorenz/lxab.bin", UNSTABLE),
    Test::Suite("Visual6502 test for adc/sbc in decimal mode."),
    Test::Run("tests/6502DecimalMode.bin", 0x8133),
    Test::Suite("Piotr Fusik tests."),
    Test::Run("tests/cpu_decimal.bin", 0x302F),
    Test::Run("tests/cpu_las.bin", 0x304F),
    Test::Suite("Avery Lee tests."),
    Test::Run("tests/avery.bin", 0x20DB),
    Test::Run("tests/avery2.bin", 0x20FA),
    Test::Skip(
        "tests/avery3.bin",
        "includes SHX and SHY, which are not emulated",
    ),
    Test::Suite("HCM6502 tests."),
    Test::Run("tests/AllSuiteA.bin", 0x45C0),
];

/// Expected cycles per instruction in `cycles.bin`, in program order from
/// `$3000`, as listed by Fake6502's `test.c`.
const EXPECTED_CYCLES: &[u32] = &[
    7, 6, 6, 3, 5, 3, 2, 2, 4, 6, 2, 2, 2, 3, 4, 2, 5, 2, 6, 4, 6, 2, 4, 5, 2, 4, 5, 7, 6, 6, 6, 3,
    3, 5, 4, 2, 2, 4, 4, 6, 2, 2, 2, 3, 4, 2, 5, 2, 6, 4, 6, 2, 4, 5, 4, 5, 7, 6, 3, 5, 3, 2, 2, 3,
    4, 6, 2, 3, 4, 2, 2, 3, 4, 2, 5, 2, 6, 4, 6, 2, 4, 5, 4, 5, 7, 2, 6, 3, 2, 4, 2, 7, 4, 3, 5, 5,
    4, 2, 5, 6, 2, 2, 2, 3, 4, 3, 4, 2, 2, 5, 2, 6, 4, 4, 5, 4, 5, 2, 2, 6, 2, 7, 5, 5, 6, 5, 6, 6,
    2, 7, 2, 6, 3, 3, 3, 2, 2, 4, 4, 4, 2, 2, 2, 3, 4, 2, 6, 2, 3, 6, 2, 4, 4, 4, 2, 5, 5, 2, 5, 5,
    2, 6, 2, 3, 3, 3, 2, 2, 2, 4, 4, 4, 2, 2, 2, 3, 4, 2, 5, 2, 6, 4, 4, 4, 2, 2, 4, 5, 2, 2, 4, 5,
    4, 5, 4, 5, 2, 6, 3, 3, 5, 2, 2, 2, 4, 4, 6, 2, 2, 2, 3, 4, 2, 5, 2, 6, 4, 6, 2, 4, 5, 4, 5, 7,
    2, 2, 6, 3, 2, 4, 2, 7, 4, 3, 5, 3, 5, 2, 2, 4, 6, 2, 2, 2, 3, 4, 2, 2, 5, 2, 6, 4, 4, 5, 2, 4,
    2, 5, 2, 2, 6, 2, 7, 5, 5, 6, 2, 5, 2, 6, 6, 2, 7, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 4,
    4, 4, 4, 4, 4, 4, 2, 4, 4, 4, 4, 4, 4, 5, 5, 5, 5, 5, 5, 5, 6, 8, 8, 6, 7, 7, 5, 6, 8, 8, 6, 7,
    7, 5, 6, 8, 8, 6, 7, 7, 5, 6, 8, 8, 6, 7, 7, 3, 4, 6, 4, 3, 4, 6, 4, 2, 5, 4, 2, 6, 5, 5, 6, 8,
    8, 6, 7, 7, 5, 6, 8, 8, 6, 7, 7, 2, 2, 2, 2, 2, 2, 4, 5, 6, 5, 5, 5, 5, 2, 5, 5, 5, 5, 2, 2, 3,
];

fn load(dir: &Path, name: &str) -> Option<(Cpu, Ram)> {
    print!("{name} -- ");
    let mut data = match std::fs::read(dir.join(name)) {
        Ok(data) if data.len() >= 65_536 => data,
        Ok(_) => {
            eprintln!("premature EOF");
            return None;
        }
        Err(error) => {
            eprintln!("cannot open test: {error}");
            return None;
        }
    };
    data.truncate(65_536);
    let mut ram = Ram(data);
    let mut cpu = Cpu::new();
    cpu.reset(&mut ram);
    Some((cpu, ram))
}

fn run(dir: &Path, name: &str, success: u16) -> bool {
    let Some((mut cpu, mut ram)) = load(dir, name) else {
        return false;
    };
    loop {
        let before = cpu.pc;
        cpu.step(&mut ram);
        if cpu.pc == before {
            let passed = cpu.pc == success;
            println!(
                "{} -- PC={:04x} A={:02x} X={:02x} Y={:02x} SP={:02x} P={:02x}",
                if passed { PASS } else { FAIL },
                cpu.pc,
                cpu.a,
                cpu.x,
                cpu.y,
                cpu.sp,
                cpu.status()
            );
            return passed;
        }
    }
}

fn cycles(dir: &Path) -> bool {
    println!("\nTest cycles per instruction.");
    let Some((mut cpu, mut ram)) = load(dir, "tests/cycles.bin") else {
        return false;
    };
    let mut compare = false;
    let mut index = 0;
    loop {
        let pc = cpu.pc;
        let opcode = ram.read(pc);
        if pc == 0x3000 {
            compare = true;
        }
        let spent = cpu.step(&mut ram);
        if compare {
            let expected = EXPECTED_CYCLES.get(index).copied().unwrap_or(0);
            if spent != expected {
                println!("PC: {pc:04X} instr: ${opcode:02x} spent: {spent} expected: {expected}");
                println!("{FAIL}");
                return false;
            }
            index += 1;
        }
        if cpu.pc == 0x200A {
            println!("{PASS}");
            return true;
        }
    }
}

fn main() -> ExitCode {
    let dir = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("host/fake6502-upstream"), PathBuf::from);
    let mut failures = 0;
    for test in TESTS {
        match test {
            Test::Suite(name) => println!("\n{name}"),
            Test::Run(name, success) => {
                if !run(&dir, name, *success) {
                    failures += 1;
                }
            }
            Test::Skip(name, reason) => println!("{name} -- {SKIP} -- {reason}"),
        }
    }
    if !cycles(&dir) {
        failures += 1;
    }
    if failures == 0 {
        println!("\nAll CPU tests passed.");
        ExitCode::SUCCESS
    } else {
        println!("\n{failures} CPU test(s) FAILED.");
        ExitCode::FAILURE
    }
}

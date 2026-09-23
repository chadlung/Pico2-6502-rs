//! MOS 6502 CPU core: a Rust port of Fake6502.
//!
//! Fake6502 is Copyright © 2011-2013 Mike Chambers and Copyright © 2024 Ivo
//! van Poorten, BSD 2-Clause License (see `LICENSE-fake6502`).  This port
//! follows <https://github.com/ivop/fake6502> commit `b52676f` with the Pico
//! 6502 change that reset sets the I flag, as an NMOS 6502 does.
//!
//! Every documented instruction and the stable undocumented ones are
//! emulated, including NMOS decimal mode.  The unstable undocumented opcodes
//! (ANE `$8B`, LXA `$AB`, SHA `$93`/`$9F`, SHX `$9E`, SHY `$9C` and TAS `$9B`)
//! are deliberately not: like the JAM opcodes, they do nothing, though they
//! still use their addressing mode's length and cycles.

/// Memory as seen by the CPU.  Every access goes through here, so reads and
/// writes of memory-mapped I/O happen exactly as the instructions make them.
pub trait Memory {
    /// Reads one byte.
    fn read(&mut self, address: u16) -> u8;
    /// Writes one byte.
    fn write(&mut self, address: u16, value: u8);
}

/// CPU registers and flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Cpu {
    /// Program counter.
    pub pc: u16,
    /// Stack pointer, an offset into page one.
    pub sp: u8,
    /// Accumulator.
    pub a: u8,
    /// X index register.
    pub x: u8,
    /// Y index register.
    pub y: u8,
    c: bool,
    z: bool,
    i: bool,
    d: bool,
    v: bool,
    n: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Imp,
    Acc,
    Imm,
    Zp,
    Zpx,
    Zpy,
    Abs,
    Rel,
    Absx,
    Absy,
    Ind,
    Indx,
    Indy,
}

#[derive(Clone, Copy)]
enum Op {
    Adc,
    And,
    Asl,
    Bcc,
    Bcs,
    Beq,
    Bit,
    Bmi,
    Bne,
    Bpl,
    Brk,
    Bvc,
    Bvs,
    Clc,
    Cld,
    Cli,
    Clv,
    Cmp,
    Cpx,
    Cpy,
    Dec,
    Dex,
    Dey,
    Eor,
    Inc,
    Inx,
    Iny,
    Jmp,
    Jsr,
    Lda,
    Ldx,
    Ldy,
    Lsr,
    Nop,
    Ora,
    Pha,
    Php,
    Pla,
    Plp,
    Rol,
    Ror,
    Rti,
    Rts,
    Sbc,
    Sec,
    Sed,
    Sei,
    Sta,
    Stx,
    Sty,
    Tax,
    Tay,
    Tsx,
    Txa,
    Txs,
    Tya,
    // Stable undocumented opcodes.
    Slo,
    Rla,
    Sre,
    Rra,
    Sax,
    Lax,
    Dcp,
    Isc,
    Anc,
    Alr,
    Arr,
    Sbx,
    Las,
    /// JAM, and the unstable opcodes this core leaves out.
    Jam,
}

#[rustfmt::skip]
const MODES: [Mode; 256] = {
    use Mode::{Abs, Absx, Absy, Acc, Imm, Imp, Ind, Indx, Indy, Rel, Zp, Zpx, Zpy};
    [
    // 0    1     2    3     4    5    6    7    8    9     A    B     C     D     E     F
      Imp, Indx, Imp, Indx, Zp,  Zp,  Zp,  Zp,  Imp, Imm,  Acc, Imm,  Abs,  Abs,  Abs,  Abs,  // 0
      Rel, Indy, Imp, Indy, Zpx, Zpx, Zpx, Zpx, Imp, Absy, Imp, Absy, Absx, Absx, Absx, Absx, // 1
      Abs, Indx, Imp, Indx, Zp,  Zp,  Zp,  Zp,  Imp, Imm,  Acc, Imm,  Abs,  Abs,  Abs,  Abs,  // 2
      Rel, Indy, Imp, Indy, Zpx, Zpx, Zpx, Zpx, Imp, Absy, Imp, Absy, Absx, Absx, Absx, Absx, // 3
      Imp, Indx, Imp, Indx, Zp,  Zp,  Zp,  Zp,  Imp, Imm,  Acc, Imm,  Abs,  Abs,  Abs,  Abs,  // 4
      Rel, Indy, Imp, Indy, Zpx, Zpx, Zpx, Zpx, Imp, Absy, Imp, Absy, Absx, Absx, Absx, Absx, // 5
      Imp, Indx, Imp, Indx, Zp,  Zp,  Zp,  Zp,  Imp, Imm,  Acc, Imm,  Ind,  Abs,  Abs,  Abs,  // 6
      Rel, Indy, Imp, Indy, Zpx, Zpx, Zpx, Zpx, Imp, Absy, Imp, Absy, Absx, Absx, Absx, Absx, // 7
      Imm, Indx, Imm, Indx, Zp,  Zp,  Zp,  Zp,  Imp, Imm,  Imp, Imm,  Abs,  Abs,  Abs,  Abs,  // 8
      Rel, Indy, Imp, Indy, Zpx, Zpx, Zpy, Zpy, Imp, Absy, Imp, Absy, Absx, Absx, Absy, Absy, // 9
      Imm, Indx, Imm, Indx, Zp,  Zp,  Zp,  Zp,  Imp, Imm,  Imp, Imm,  Abs,  Abs,  Abs,  Abs,  // A
      Rel, Indy, Imp, Indy, Zpx, Zpx, Zpy, Zpy, Imp, Absy, Imp, Absy, Absx, Absx, Absy, Absy, // B
      Imm, Indx, Imm, Indx, Zp,  Zp,  Zp,  Zp,  Imp, Imm,  Imp, Imm,  Abs,  Abs,  Abs,  Abs,  // C
      Rel, Indy, Imp, Indy, Zpx, Zpx, Zpx, Zpx, Imp, Absy, Imp, Absy, Absx, Absx, Absx, Absx, // D
      Imm, Indx, Imm, Indx, Zp,  Zp,  Zp,  Zp,  Imp, Imm,  Imp, Imm,  Abs,  Abs,  Abs,  Abs,  // E
      Rel, Indy, Imp, Indy, Zpx, Zpx, Zpx, Zpx, Imp, Absy, Imp, Absy, Absx, Absx, Absx, Absx, // F
    ]
};

// Unstable opcodes, mapped to Jam: $8B ANE, $93 SHA, $9B TAS, $9C SHY,
// $9E SHX, $9F SHA, $AB LXA.
#[rustfmt::skip]
const OPS: [Op; 256] = {
    use Op::{
        Adc, Alr, Anc, And, Arr, Asl, Bcc, Bcs, Beq, Bit, Bmi, Bne, Bpl, Brk, Bvc, Bvs, Clc,
        Cld, Cli, Clv, Cmp, Cpx, Cpy, Dcp, Dec, Dex, Dey, Eor, Inc, Inx, Iny, Isc, Jam, Jmp,
        Jsr, Las, Lax, Lda, Ldx, Ldy, Lsr, Nop, Ora, Pha, Php, Pla, Plp, Rla, Rol, Ror, Rra,
        Rti, Rts, Sax, Sbc, Sbx, Sec, Sed, Sei, Slo, Sre, Sta, Stx, Sty, Tax, Tay, Tsx, Txa,
        Txs, Tya,
    };
    [
    // 0    1    2    3    4    5    6    7    8    9    A    B    C    D    E    F
      Brk, Ora, Jam, Slo, Nop, Ora, Asl, Slo, Php, Ora, Asl, Anc, Nop, Ora, Asl, Slo, // 0
      Bpl, Ora, Jam, Slo, Nop, Ora, Asl, Slo, Clc, Ora, Nop, Slo, Nop, Ora, Asl, Slo, // 1
      Jsr, And, Jam, Rla, Bit, And, Rol, Rla, Plp, And, Rol, Anc, Bit, And, Rol, Rla, // 2
      Bmi, And, Jam, Rla, Nop, And, Rol, Rla, Sec, And, Nop, Rla, Nop, And, Rol, Rla, // 3
      Rti, Eor, Jam, Sre, Nop, Eor, Lsr, Sre, Pha, Eor, Lsr, Alr, Jmp, Eor, Lsr, Sre, // 4
      Bvc, Eor, Jam, Sre, Nop, Eor, Lsr, Sre, Cli, Eor, Nop, Sre, Nop, Eor, Lsr, Sre, // 5
      Rts, Adc, Jam, Rra, Nop, Adc, Ror, Rra, Pla, Adc, Ror, Arr, Jmp, Adc, Ror, Rra, // 6
      Bvs, Adc, Jam, Rra, Nop, Adc, Ror, Rra, Sei, Adc, Nop, Rra, Nop, Adc, Ror, Rra, // 7
      Nop, Sta, Nop, Sax, Sty, Sta, Stx, Sax, Dey, Nop, Txa, Jam, Sty, Sta, Stx, Sax, // 8
      Bcc, Sta, Jam, Jam, Sty, Sta, Stx, Sax, Tya, Sta, Txs, Jam, Jam, Sta, Jam, Jam, // 9
      Ldy, Lda, Ldx, Lax, Ldy, Lda, Ldx, Lax, Tay, Lda, Tax, Jam, Ldy, Lda, Ldx, Lax, // A
      Bcs, Lda, Jam, Lax, Ldy, Lda, Ldx, Lax, Clv, Lda, Tsx, Las, Ldy, Lda, Ldx, Lax, // B
      Cpy, Cmp, Nop, Dcp, Cpy, Cmp, Dec, Dcp, Iny, Cmp, Dex, Sbx, Cpy, Cmp, Dec, Dcp, // C
      Bne, Cmp, Jam, Dcp, Nop, Cmp, Dec, Dcp, Cld, Cmp, Nop, Dcp, Nop, Cmp, Dec, Dcp, // D
      Cpx, Sbc, Nop, Isc, Cpx, Sbc, Inc, Isc, Inx, Sbc, Nop, Sbc, Cpx, Sbc, Inc, Isc, // E
      Beq, Sbc, Jam, Isc, Nop, Sbc, Inc, Isc, Sed, Sbc, Nop, Isc, Nop, Sbc, Inc, Isc, // F
    ]
};

#[rustfmt::skip]
const TICKS: [u8; 256] = [
//  0  1  2  3  4  5  6  7  8  9  A  B  C  D  E  F
    7, 6, 2, 8, 3, 3, 5, 5, 3, 2, 2, 2, 4, 4, 6, 6, // 0
    2, 5, 2, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7, // 1
    6, 6, 2, 8, 3, 3, 5, 5, 4, 2, 2, 2, 4, 4, 6, 6, // 2
    2, 5, 2, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7, // 3
    6, 6, 2, 8, 3, 3, 5, 5, 3, 2, 2, 2, 3, 4, 6, 6, // 4
    2, 5, 2, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7, // 5
    6, 6, 2, 8, 3, 3, 5, 5, 4, 2, 2, 2, 5, 4, 6, 6, // 6
    2, 5, 2, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7, // 7
    2, 6, 2, 6, 3, 3, 3, 3, 2, 2, 2, 2, 4, 4, 4, 4, // 8
    2, 6, 2, 6, 4, 4, 4, 4, 2, 5, 2, 5, 5, 5, 5, 5, // 9
    2, 6, 2, 6, 3, 3, 3, 3, 2, 2, 2, 2, 4, 4, 4, 4, // A
    2, 5, 2, 5, 4, 4, 4, 4, 2, 4, 2, 4, 4, 4, 4, 4, // B
    2, 6, 2, 8, 3, 3, 5, 5, 2, 2, 2, 2, 4, 4, 6, 6, // C
    2, 5, 2, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7, // D
    2, 6, 2, 8, 3, 3, 5, 5, 2, 2, 2, 2, 4, 4, 6, 6, // E
    2, 5, 2, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7, // F
];

const VEC_NMI: u16 = 0xFFFA;
const VEC_RESET: u16 = 0xFFFC;
const VEC_IRQ: u16 = 0xFFFE;

impl Cpu {
    /// Creates a CPU with every register and flag clear.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pc: 0,
            sp: 0,
            a: 0,
            x: 0,
            y: 0,
            c: false,
            z: false,
            i: false,
            d: false,
            v: false,
            n: false,
        }
    }

    /// The status register as `NV-BDIZC`.  The unused bit reads as 1 and
    /// B as 0, as `PHP` and interrupts see them before setting B.
    #[must_use]
    pub const fn status(&self) -> u8 {
        (self.n as u8) << 7
            | (self.v as u8) << 6
            | 1 << 5
            | (self.d as u8) << 3
            | (self.i as u8) << 2
            | (self.z as u8) << 1
            | self.c as u8
    }

    /// Sets the flags from a status byte; bits 4 and 5 are ignored.
    pub const fn set_status(&mut self, p: u8) {
        self.n = p & 0x80 != 0;
        self.v = p & 0x40 != 0;
        self.d = p & 0x08 != 0;
        self.i = p & 0x04 != 0;
        self.z = p & 0x02 != 0;
        self.c = p & 0x01 != 0;
    }

    /// Resets the CPU: PC from the reset vector, A, X, Y and the flags clear
    /// except I, and SP at `$FD`.  Returns the cycles taken.
    pub fn reset(&mut self, memory: &mut impl Memory) -> u32 {
        self.pc = read_word(memory, VEC_RESET);
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.set_status(0);
        self.i = true;
        self.sp = 0xFD;
        7
    }

    /// Takes a non-maskable interrupt.  Returns the cycles taken.
    pub fn nmi(&mut self, memory: &mut impl Memory) -> u32 {
        self.interrupt(memory, VEC_NMI)
    }

    /// Takes an interrupt request, whatever the I flag.  Returns the cycles
    /// taken.
    pub fn irq(&mut self, memory: &mut impl Memory) -> u32 {
        self.interrupt(memory, VEC_IRQ)
    }

    fn interrupt(&mut self, memory: &mut impl Memory, vector: u16) -> u32 {
        let mut step = Step::new(self, memory, 0);
        step.push16(step.cpu.pc);
        step.push8(step.cpu.status());
        step.cpu.i = true;
        step.cpu.pc = read_word(step.memory, vector);
        7
    }

    /// Executes one instruction.  Returns the cycles it took.  The firmware
    /// runs this from RAM, for speed.
    #[cfg_attr(
        feature = "firmware",
        allow(unsafe_code),
        unsafe(link_section = ".data.ram_func")
    )]
    pub fn step(&mut self, memory: &mut impl Memory) -> u32 {
        let opcode = memory.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        let mut step = Step::new(self, memory, opcode);
        step.address();
        step.execute();
        if step.penalty_op && step.penalty_addr {
            step.ticks += 1;
        }
        step.ticks
    }
}

#[inline]
fn read_word(memory: &mut impl Memory, address: u16) -> u16 {
    u16::from_le_bytes([memory.read(address), memory.read(address.wrapping_add(1))])
}

/// State while one instruction executes, as Fake6502 keeps it in globals.
struct Step<'a, M> {
    cpu: &'a mut Cpu,
    memory: &'a mut M,
    opcode: u8,
    mode: Mode,
    ea: u16,
    ticks: u32,
    penalty_op: bool,
    penalty_addr: bool,
}

// The helpers are small and run for every instruction; inlining them keeps
// the firmware's inner loop in RAM.
#[allow(clippy::inline_always)]
impl<'a, M: Memory> Step<'a, M> {
    fn new(cpu: &'a mut Cpu, memory: &'a mut M, opcode: u8) -> Self {
        let index = usize::from(opcode);
        Self {
            cpu,
            memory,
            opcode,
            mode: MODES[index],
            ea: 0,
            ticks: u32::from(TICKS[index]),
            penalty_op: false,
            penalty_addr: false,
        }
    }

    // ------------------ Memory and stack -----------------------------------

    #[inline(always)]
    fn read(&mut self, address: u16) -> u8 {
        self.memory.read(address)
    }

    #[inline(always)]
    fn write(&mut self, address: u16, value: u8) {
        self.memory.write(address, value);
    }

    #[inline(always)]
    fn fetch(&mut self) -> u8 {
        let value = self.read(self.cpu.pc);
        self.cpu.pc = self.cpu.pc.wrapping_add(1);
        value
    }

    #[inline(always)]
    fn fetch_word(&mut self) -> u16 {
        let word = read_word(self.memory, self.cpu.pc);
        self.cpu.pc = self.cpu.pc.wrapping_add(2);
        word
    }

    #[inline(always)]
    fn push16(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        let sp = self.cpu.sp;
        self.write(0x0100 | u16::from(sp), hi);
        self.write(0x0100 | u16::from(sp.wrapping_sub(1)), lo);
        self.cpu.sp = sp.wrapping_sub(2);
    }

    #[inline(always)]
    fn push8(&mut self, value: u8) {
        self.write(0x0100 | u16::from(self.cpu.sp), value);
        self.cpu.sp = self.cpu.sp.wrapping_sub(1);
    }

    #[inline(always)]
    fn pull8(&mut self) -> u8 {
        self.cpu.sp = self.cpu.sp.wrapping_add(1);
        self.read(0x0100 | u16::from(self.cpu.sp))
    }

    #[inline(always)]
    fn pull16(&mut self) -> u16 {
        self.cpu.sp = self.cpu.sp.wrapping_add(2);
        let sp = self.cpu.sp;
        let lo = self.read(0x0100 | u16::from(sp.wrapping_sub(1)));
        let hi = self.read(0x0100 | u16::from(sp));
        u16::from_le_bytes([lo, hi])
    }

    // ------------------ Addressing modes -----------------------------------

    #[cfg_attr(
        feature = "firmware",
        allow(unsafe_code),
        unsafe(link_section = ".data.ram_func")
    )]
    fn address(&mut self) {
        match self.mode {
            Mode::Imp | Mode::Acc => {}
            Mode::Imm => {
                self.ea = self.cpu.pc;
                self.cpu.pc = self.cpu.pc.wrapping_add(1);
            }
            Mode::Zp => self.ea = u16::from(self.fetch()),
            Mode::Zpx => self.ea = u16::from(self.fetch().wrapping_add(self.cpu.x)),
            Mode::Zpy => self.ea = u16::from(self.fetch().wrapping_add(self.cpu.y)),
            Mode::Abs => self.ea = self.fetch_word(),
            Mode::Rel => {
                let next = self.cpu.pc.wrapping_add(1);
                let offset = i8::from_ne_bytes([self.fetch()]);
                self.ea = next.wrapping_add_signed(i16::from(offset));
            }
            Mode::Absx => self.indexed(self.cpu.x),
            Mode::Absy => self.indexed(self.cpu.y),
            Mode::Ind => {
                let pointer = self.fetch_word();
                // The page-wrap bug: ($10FF) reads $10FF and $1000.
                let high = (pointer & 0xFF00) | (pointer.wrapping_add(1) & 0x00FF);
                self.ea = u16::from_le_bytes([self.read(pointer), self.read(high)]);
            }
            Mode::Indx => {
                let pointer = self.fetch().wrapping_add(self.cpu.x);
                self.ea = self.zero_page_word(pointer);
            }
            Mode::Indy => {
                let pointer = self.fetch();
                let base = self.zero_page_word(pointer);
                self.ea = base.wrapping_add(u16::from(self.cpu.y));
                self.penalty_addr = base & 0xFF00 != self.ea & 0xFF00;
            }
        }
    }

    #[inline(always)]
    fn indexed(&mut self, index: u8) {
        let base = self.fetch_word();
        self.ea = base.wrapping_add(u16::from(index));
        self.penalty_addr = base & 0xFF00 != self.ea & 0xFF00;
    }

    #[inline(always)]
    fn zero_page_word(&mut self, pointer: u8) -> u16 {
        let lo = self.read(u16::from(pointer));
        let hi = self.read(u16::from(pointer.wrapping_add(1)));
        u16::from_le_bytes([lo, hi])
    }

    #[inline(always)]
    fn get_value(&mut self) -> u8 {
        if self.mode == Mode::Acc {
            self.cpu.a
        } else {
            self.read(self.ea)
        }
    }

    #[inline(always)]
    fn put_value(&mut self, value: u8) {
        if self.mode == Mode::Acc {
            self.cpu.a = value;
        } else {
            self.write(self.ea, value);
        }
    }

    // ------------------ Flags ----------------------------------------------

    #[inline(always)]
    const fn calc_zn(&mut self, value: u8) {
        self.cpu.z = value == 0;
        self.cpu.n = value & 0x80 != 0;
    }

    /// Carry from bit 8 or above, then Z and N from the low byte.
    #[inline(always)]
    const fn calc_czn(&mut self, result: u16) {
        self.cpu.c = result & 0xFF00 != 0;
        self.calc_zn(result.to_le_bytes()[0]);
    }

    #[inline(always)]
    const fn calc_v(&mut self, result: u16, accumulator: u8, value: u16) {
        self.cpu.v = (result ^ accumulator as u16) & (result ^ value) & 0x80 != 0;
    }

    // ------------------ Instructions ---------------------------------------

    #[allow(clippy::too_many_lines)]
    #[cfg_attr(
        feature = "firmware",
        allow(unsafe_code),
        unsafe(link_section = ".data.ram_func")
    )]
    fn execute(&mut self) {
        match OPS[usize::from(self.opcode)] {
            Op::Adc => self.adc(),
            Op::And => self.and(),
            Op::Asl => self.asl(),
            Op::Bcc => self.branch(!self.cpu.c),
            Op::Bcs => self.branch(self.cpu.c),
            Op::Beq => self.branch(self.cpu.z),
            Op::Bit => {
                let value = self.get_value();
                self.cpu.z = self.cpu.a & value == 0;
                self.cpu.n = value & 0x80 != 0;
                self.cpu.v = value & 0x40 != 0;
            }
            Op::Bmi => self.branch(self.cpu.n),
            Op::Bne => self.branch(!self.cpu.z),
            Op::Bpl => self.branch(!self.cpu.n),
            Op::Brk => {
                self.cpu.pc = self.cpu.pc.wrapping_add(1);
                self.push16(self.cpu.pc);
                self.php();
                self.cpu.i = true;
                self.cpu.pc = read_word(self.memory, VEC_IRQ);
            }
            Op::Bvc => self.branch(!self.cpu.v),
            Op::Bvs => self.branch(self.cpu.v),
            Op::Clc => self.cpu.c = false,
            Op::Cld => self.cpu.d = false,
            Op::Cli => self.cpu.i = false,
            Op::Clv => self.cpu.v = false,
            Op::Cmp => self.cmp(),
            Op::Cpx => {
                let value = self.get_value();
                self.compare(self.cpu.x, value);
            }
            Op::Cpy => {
                let value = self.get_value();
                self.compare(self.cpu.y, value);
            }
            Op::Dec => self.dec(),
            Op::Dex => {
                self.cpu.x = self.cpu.x.wrapping_sub(1);
                self.calc_zn(self.cpu.x);
            }
            Op::Dey => {
                self.cpu.y = self.cpu.y.wrapping_sub(1);
                self.calc_zn(self.cpu.y);
            }
            Op::Eor => self.eor(),
            Op::Inc => self.inc(),
            Op::Inx => {
                self.cpu.x = self.cpu.x.wrapping_add(1);
                self.calc_zn(self.cpu.x);
            }
            Op::Iny => {
                self.cpu.y = self.cpu.y.wrapping_add(1);
                self.calc_zn(self.cpu.y);
            }
            Op::Jmp => self.cpu.pc = self.ea,
            Op::Jsr => {
                self.push16(self.cpu.pc.wrapping_sub(1));
                self.cpu.pc = self.ea;
            }
            Op::Lda => self.lda(),
            Op::Ldx => self.ldx(),
            Op::Ldy => {
                self.penalty_op = true;
                self.cpu.y = self.get_value();
                self.calc_zn(self.cpu.y);
            }
            Op::Lsr => self.lsr(),
            Op::Nop => self.nop(),
            Op::Ora => self.ora(),
            Op::Pha => self.push8(self.cpu.a),
            Op::Php => self.php(),
            Op::Pla => {
                self.cpu.a = self.pull8();
                self.calc_zn(self.cpu.a);
            }
            Op::Plp => {
                let p = self.pull8();
                self.cpu.set_status(p);
            }
            Op::Rol => self.rol(),
            Op::Ror => self.ror(),
            Op::Rti => {
                let p = self.pull8();
                self.cpu.set_status(p);
                self.cpu.pc = self.pull16();
            }
            Op::Rts => self.cpu.pc = self.pull16().wrapping_add(1),
            Op::Sbc => self.sbc(),
            Op::Sec => self.cpu.c = true,
            Op::Sed => self.cpu.d = true,
            Op::Sei => self.cpu.i = true,
            Op::Sta => self.put_value(self.cpu.a),
            Op::Stx => self.put_value(self.cpu.x),
            Op::Sty => self.put_value(self.cpu.y),
            Op::Tax => {
                self.cpu.x = self.cpu.a;
                self.calc_zn(self.cpu.x);
            }
            Op::Tay => {
                self.cpu.y = self.cpu.a;
                self.calc_zn(self.cpu.y);
            }
            Op::Tsx => {
                self.cpu.x = self.cpu.sp;
                self.calc_zn(self.cpu.x);
            }
            Op::Txa => {
                self.cpu.a = self.cpu.x;
                self.calc_zn(self.cpu.a);
            }
            Op::Txs => self.cpu.sp = self.cpu.x,
            Op::Tya => {
                self.cpu.a = self.cpu.y;
                self.calc_zn(self.cpu.a);
            }
            Op::Slo => {
                self.asl();
                self.ora();
            }
            Op::Rla => {
                self.rol();
                self.and();
                self.penalty_op = false;
            }
            Op::Sre => {
                self.lsr();
                self.eor();
                self.penalty_op = false;
            }
            Op::Rra => {
                self.ror();
                self.adc();
                self.penalty_op = false;
                if self.cpu.d {
                    self.ticks -= 1;
                }
            }
            Op::Sax => self.put_value(self.cpu.a & self.cpu.x),
            Op::Lax => {
                self.penalty_op = true;
                self.lda();
                self.ldx();
            }
            Op::Dcp => {
                self.dec();
                self.cmp();
                self.penalty_op = false;
            }
            Op::Isc => {
                self.inc();
                self.sbc();
                self.penalty_op = false;
                if self.cpu.d {
                    self.ticks -= 1;
                }
            }
            Op::Anc => {
                self.and();
                self.cpu.c = self.cpu.a & 0x80 != 0;
            }
            Op::Alr => {
                self.and();
                self.cpu.c = self.cpu.a & 1 != 0;
                self.cpu.a >>= 1;
                self.calc_zn(self.cpu.a);
            }
            Op::Arr => self.arr(),
            Op::Sbx => {
                let value = self.get_value();
                self.cpu.x &= self.cpu.a;
                self.compare(self.cpu.x, value);
                self.cpu.x = self.cpu.x.wrapping_sub(value);
            }
            Op::Las => {
                self.penalty_op = true;
                let value = self.get_value() & self.cpu.sp;
                self.cpu.sp = value;
                self.cpu.a = value;
                self.cpu.x = value;
                self.calc_zn(value);
            }
            Op::Jam => {}
        }
    }

    #[inline(always)]
    fn branch(&mut self, condition: bool) {
        if condition {
            let old_pc = self.cpu.pc;
            self.cpu.pc = self.ea;
            self.ticks += if old_pc & 0xFF00 == self.cpu.pc & 0xFF00 {
                1
            } else {
                2
            };
        }
    }

    #[inline(always)]
    fn nop(&mut self) {
        if matches!(self.opcode, 0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC) {
            self.penalty_op = true;
        }
    }

    #[inline(always)]
    fn and(&mut self) {
        self.penalty_op = true;
        self.cpu.a &= self.get_value();
        self.calc_zn(self.cpu.a);
    }

    #[inline(always)]
    fn eor(&mut self) {
        self.penalty_op = true;
        self.cpu.a ^= self.get_value();
        self.calc_zn(self.cpu.a);
    }

    #[inline(always)]
    fn ora(&mut self) {
        self.penalty_op = true;
        self.cpu.a |= self.get_value();
        self.calc_zn(self.cpu.a);
    }

    #[inline(always)]
    fn lda(&mut self) {
        self.penalty_op = true;
        self.cpu.a = self.get_value();
        self.calc_zn(self.cpu.a);
    }

    #[inline(always)]
    fn ldx(&mut self) {
        self.penalty_op = true;
        self.cpu.x = self.get_value();
        self.calc_zn(self.cpu.x);
    }

    #[inline(always)]
    const fn compare(&mut self, register: u8, value: u8) {
        self.cpu.n = register.wrapping_sub(value) & 0x80 != 0;
        self.cpu.c = register >= value;
        self.cpu.z = register == value;
    }

    #[inline(always)]
    fn cmp(&mut self) {
        let value = self.get_value();
        self.compare(self.cpu.a, value);
        self.penalty_op = true;
    }

    #[inline(always)]
    fn php(&mut self) {
        self.push8(self.cpu.status() | 0x10);
    }

    #[inline(always)]
    fn dec(&mut self) {
        let result = self.get_value().wrapping_sub(1);
        self.calc_zn(result);
        self.put_value(result);
    }

    #[inline(always)]
    fn inc(&mut self) {
        let result = self.get_value().wrapping_add(1);
        self.calc_zn(result);
        self.put_value(result);
    }

    #[inline(always)]
    fn asl(&mut self) {
        let result = u16::from(self.get_value()) << 1;
        self.calc_czn(result);
        self.put_value(result.to_le_bytes()[0]);
    }

    #[inline(always)]
    fn lsr(&mut self) {
        let value = self.get_value();
        let result = value >> 1;
        self.cpu.c = value & 1 != 0;
        self.calc_zn(result);
        self.put_value(result);
    }

    #[inline(always)]
    fn rol(&mut self) {
        let result = u16::from(self.get_value()) << 1 | u16::from(self.cpu.c);
        self.calc_czn(result);
        self.put_value(result.to_le_bytes()[0]);
    }

    #[inline(always)]
    fn ror(&mut self) {
        let value = self.get_value();
        let result = value >> 1 | u8::from(self.cpu.c) << 7;
        self.cpu.c = value & 1 != 0;
        self.calc_zn(result);
        self.put_value(result);
    }

    #[inline(always)]
    fn adc(&mut self) {
        self.penalty_op = true;
        let a = self.cpu.a;
        let value = u16::from(self.get_value());
        let carry = u16::from(self.cpu.c);
        let mut result = u16::from(a) + value + carry;
        // NMOS: Z comes from the binary sum, even in decimal mode.
        self.cpu.z = result.to_le_bytes()[0] == 0;

        if self.cpu.d {
            result = (u16::from(a) & 0x0F) + (value & 0x0F) + carry;
            if result >= 0x0A {
                result = ((result + 0x06) & 0x0F) + 0x10;
            }
            result += (u16::from(a) & 0xF0) + (value & 0xF0);
            self.cpu.n = result & 0x80 != 0;
            self.calc_v(result, a, value);
            if result >= 0xA0 {
                result += 0x60;
            }
            self.cpu.c = result & 0xFF00 != 0;
            self.ticks += 1;
        } else {
            self.cpu.c = result & 0xFF00 != 0;
            self.calc_v(result, a, value);
            self.cpu.n = result & 0x80 != 0;
        }

        self.cpu.a = result.to_le_bytes()[0];
    }

    #[inline(always)]
    fn sbc(&mut self) {
        let old_carry = u16::from(self.cpu.c);
        self.penalty_op = true;
        let a = u16::from(self.cpu.a);
        let value = u16::from(self.get_value() ^ 0xFF);
        let mut result = a + value + old_carry;
        self.calc_czn(result);
        self.calc_v(result, self.cpu.a, value);

        if self.cpu.d {
            let b = value ^ 0xFF;
            let mut low = (a & 0x0F)
                .wrapping_sub(b & 0x0F)
                .wrapping_add(old_carry)
                .wrapping_sub(1);
            if low & 0x8000 != 0 {
                low = (low.wrapping_sub(0x06) & 0x0F).wrapping_sub(0x10);
            }
            result = (a & 0xF0).wrapping_sub(b & 0xF0).wrapping_add(low);
            if result & 0x8000 != 0 {
                result = result.wrapping_sub(0x60);
            }
            self.ticks += 1;
        }

        self.cpu.a = result.to_le_bytes()[0];
    }

    #[inline(always)]
    fn arr(&mut self) {
        self.and();
        let before = self.cpu.a;
        self.cpu.a = self.cpu.a >> 1 | u8::from(self.cpu.c) << 7;
        self.calc_zn(self.cpu.a);

        if self.cpu.d {
            self.cpu.v = (self.cpu.a ^ before) & 0x40 != 0;
            if (before & 0x0F) + (before & 0x01) > 0x05 {
                self.cpu.a = (self.cpu.a & 0xF0) | (self.cpu.a.wrapping_add(0x06) & 0x0F);
            }
            if u16::from(before) + u16::from(before & 0x10) >= 0x60 {
                self.cpu.a = self.cpu.a.wrapping_add(0x60);
                self.cpu.c = true;
            } else {
                self.cpu.c = false;
            }
        } else {
            self.cpu.c = self.cpu.a & 0x40 != 0;
            self.cpu.v = self.cpu.c ^ (self.cpu.a >> 5 & 1 != 0);
        }
    }
}

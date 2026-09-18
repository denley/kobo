//! The CPU and bus a capture runs on, and how its routines are called.

use super::ExpandError;
use crate::cpu::smw_bus::SmwBus;
use crate::cpu::{Cpu, CpuError, Flags};
use crate::rom::Rom;

/// Instruction limit for the game's own loading and per-frame routines.
const STEP_LIMIT: u64 = 200_000_000;
/// Instruction limit for the short lookups a tool hooks into the game.
pub(super) const LOOKUP_STEP_LIMIT: u64 = 100_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Return {
    Rtl,
    Rts,
}

/// One call into the ROM: the routine and the register state its callers
/// give it. Every call starts from freshly reset registers (8-bit
/// accumulator and index, data bank and direct page zero) plus whatever
/// is set here, so no routine sees what the previous one left behind.
#[derive(Clone, Copy, Debug)]
pub(super) struct Call {
    addr: u32,
    returns: Return,
    wide_accumulator: bool,
    wide_index: bool,
    data_bank: u8,
    accumulator: u16,
    limit: u64,
}

impl Call {
    const fn new(addr: u32, returns: Return) -> Self {
        Self {
            addr,
            returns,
            wide_accumulator: false,
            wide_index: false,
            data_bank: 0,
            accumulator: 0,
            limit: STEP_LIMIT,
        }
    }

    /// A routine that returns with `RTL`.
    pub const fn jsl(addr: u32) -> Self {
        Self::new(addr, Return::Rtl)
    }

    /// A routine that returns with `RTS`.
    pub const fn jsr(addr: u32) -> Self {
        Self::new(addr, Return::Rts)
    }

    /// Enters with a 16-bit accumulator.
    pub const fn wide_accumulator(mut self) -> Self {
        self.wide_accumulator = true;
        self
    }

    /// Enters with 16-bit index registers.
    pub const fn wide_index(mut self) -> Self {
        self.wide_index = true;
        self
    }

    pub const fn data_bank(mut self, bank: u8) -> Self {
        self.data_bank = bank;
        self
    }

    pub const fn accumulator(mut self, value: u16) -> Self {
        self.accumulator = value;
        self
    }

    pub const fn limit(mut self, limit: u64) -> Self {
        self.limit = limit;
        self
    }
}

/// A CPU on a bus, loading or running one level.
pub(super) struct Machine<'r> {
    pub cpu: Cpu,
    pub bus: SmwBus<'r>,
    pub level: u16,
}

impl<'r> Machine<'r> {
    pub fn new(rom: &'r Rom, level: u16) -> Self {
        Self {
            cpu: Cpu::new(),
            bus: SmwBus::new(rom),
            level,
        }
    }

    /// Runs a routine to its return. The registers it left are in `cpu`.
    pub fn try_call(&mut self, call: Call) -> Result<(), CpuError> {
        self.cpu.reset_registers();
        if call.wide_accumulator {
            self.cpu.p &= !Flags::M;
        }
        if call.wide_index {
            self.cpu.p &= !Flags::X;
        }
        self.cpu.db = call.data_bank;
        self.cpu.a = call.accumulator;
        match call.returns {
            Return::Rtl => self.cpu.call(&mut self.bus, call.addr, call.limit),
            Return::Rts => self.cpu.call_jsr(&mut self.bus, call.addr, call.limit),
        }
    }

    /// [`Machine::try_call`] for routines the level cannot load without.
    pub fn call(&mut self, call: Call) -> Result<(), ExpandError> {
        self.try_call(call).map_err(|source| self.error(source))
    }

    /// Resets the registers and runs from `start` until the program
    /// counter reaches `stop`.
    pub fn run_until(&mut self, start: u32, stop: u32, limit: u64) -> Result<(), ExpandError> {
        self.cpu.reset_registers();
        self.resume_until(start, stop, limit)
    }

    /// Runs from `start` to `stop` with the registers as they are.
    pub fn resume_until(&mut self, start: u32, stop: u32, limit: u64) -> Result<(), ExpandError> {
        self.cpu
            .run_until(&mut self.bus, start, stop, limit)
            .map_err(|source| self.error(source))
    }

    fn error(&self, source: CpuError) -> ExpandError {
        ExpandError::Cpu {
            level: self.level,
            source,
        }
    }
}

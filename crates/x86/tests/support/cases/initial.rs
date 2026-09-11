use wasm86_x86::StatusFlags;

use super::{InitialFlags, InitialState};
use crate::support::guest::Machine;

impl InitialState {
    pub(in crate::support) fn new(flags: InitialFlags) -> Self {
        Self {
            eip: 0x1000,
            instruction_count: u32::MAX,
            flags,
            registers: Vec::new(),
            memory: Vec::new(),
            mappings: Vec::new(),
            backing: Vec::new(),
        }
    }

    pub(in crate::support) fn machine(&self, code: &[u8]) -> Machine {
        let mut machine = Machine::with_mappings(self.eip, code, &self.mappings);
        machine.cpu.instruction_count = self.instruction_count;
        for &(register, value) in &self.registers {
            machine.cpu.registers[register] = value;
        }
        if let Some(flags) = self.flags.logical() {
            machine.cpu.flags.kind = 0;
            machine.cpu.flags.status = StatusFlags {
                cf: u8::from(flags.cf),
                pf: u8::from(flags.pf),
                af: u8::from(flags.af),
                zf: u8::from(flags.zf),
                sf: u8::from(flags.sf),
                of: u8::from(flags.of),
            };
        }
        if let Some(record) = self.flags.record() {
            machine.cpu.flags = record;
        }
        for region in &self.memory {
            machine.memory(region.address, &region.bytes, region.permissions);
        }
        for (offset, bytes) in &self.backing {
            machine.backing(*offset, bytes);
        }
        machine
    }
}

use super::{image, Operation, OPERATIONS};
use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};
use wasm86_x86::{CpuState, Gpr32, StatusFlags};

struct Expected {
    destination: u32,
    flags: StatusFlags,
}

impl Expected {
    fn apply(&self, cpu: &mut CpuState, destination: Gpr32) {
        cpu.registers[destination] = self.destination;
        cpu.flags.kind = 0;
        cpu.flags.status = self.flags;
    }
}

// Enumerate bit positions with integer division, independently of clz/ctz.
// Scan parity covers every bit of the logical source operand.
fn expected(operation: Operation, bits: u32, source: u32, previous: u32) -> Expected {
    let source = u64::from(source) % 2_u64.pow(bits);
    let mut positions = (0..bits).filter(|bit| source / 2_u64.pow(*bit) % 2 != 0);
    let index = match operation {
        Operation::Bsf => positions.next(),
        Operation::Bsr => positions.next_back(),
    };
    let destination = match index {
        None => previous,
        Some(index) if bits == 16 => (previous & 0xffff_0000) | index,
        Some(index) => index,
    };
    let odd_bits = (0..bits)
        .map(|bit| source / 2_u64.pow(bit) % 2)
        .sum::<u64>()
        % 2;
    Expected {
        destination,
        flags: StatusFlags {
            cf: 0,
            pf: (1 - odd_bits) as u8,
            af: 0,
            zf: u8::from(index.is_none()),
            sf: 0,
            of: 0,
        },
    }
}

fn retire(cpu: &mut CpuState, length: u32) -> Step<'static> {
    cpu.eip = cpu.eip.wrapping_add(length);
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    Step {
        cpu: *cpu,
        ram: &[],
        exit: Exit::Dispatch(cpu.eip),
    }
}

#[test]
fn every_source_bit_agrees_with_an_independent_integer_division_model() {
    for bits in [16, 32] {
        let mut sources = vec![
            0,
            u32::MAX,
            0xffff_0000,
            0x8000_0000,
            0x8001_0000,
            0x8000_8000,
            0x8000_8001,
            0x8000_2408,
        ];
        let upper = if bits == 16 { 0xa55a_0000 } else { 0 };
        sources.extend((0..bits).map(|bit| upper | (1 << bit)));
        for operation in OPERATIONS {
            for &source in &sources {
                let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                code.extend_from_slice(&[0x0f, operation.opcode(), 0xc2]);
                let mut image = image(&code);
                image.cpu.registers.edx = source;
                let mut cpu = image.cpu;
                expected(operation, bits, source, cpu.registers.eax).apply(&mut cpu, Gpr32::Eax);
                let step = retire(&mut cpu, code.len() as u32);
                both(
                    TestModule::interpreter(),
                    &format!("{operation:?} {bits}-bit source {source:08x}"),
                    &code,
                    1,
                    &image,
                    &[step],
                );
            }
        }
    }
}

use crate::support::cases::Flags;
use wasm86_x86::CpuState;
use wasm86_x86::FlagBytes;

use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{image, prior_flags, Operation, OPERATIONS};

#[test]
fn independent_arithmetic_model_checks_register_and_immediate_indexes() {
    for bits in [16, 32] {
        let mask = u32::MAX >> (32 - bits);
        let upper = if bits == 16 { 0x4433_0000 } else { 0 };
        for operation in OPERATIONS {
            for value in [0, mask, (1 << (bits - 1)) | 1, 0xa55a_5aa5 & mask] {
                for (immediate, indexes) in [
                    (true, &[0, 1, 15, 16, 31, 32, 63, 127, 128, 255][..]),
                    (
                        false,
                        &[
                            0,
                            1,
                            15,
                            16,
                            17,
                            31,
                            32,
                            33,
                            0x7fff,
                            0x8000,
                            0xffff,
                            0x4321_8000,
                            0x1234_7fff,
                            0xffff_0001,
                            0x8000_0000,
                            u32::MAX,
                        ][..],
                    ),
                ] {
                    for &index in indexes {
                        let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                        if immediate {
                            code.extend_from_slice(&[
                                0x0f,
                                0xba,
                                0xc0 | (operation.extension() << 3),
                                index as u8,
                            ]);
                        } else {
                            code.extend_from_slice(&[0x0f, operation.register_opcode(), 0xd0]);
                        }
                        let mut image = image(&code);
                        image.cpu.registers.eax = upper | value;
                        image.cpu.registers.edx = index;
                        let result = expected(operation, bits, value, index);
                        let mut cpu = image.cpu;
                        cpu.registers.eax = upper | result.value;
                        result.apply_flags(&mut cpu, prior_flags());
                        cpu.eip += code.len() as u32;
                        cpu.instruction_count = 0;
                        both(
                            TestModule::interpreter(),
                            &format!(
                                "{operation:?} {bits}-bit {value:x}, index {index:x}, immediate {immediate}"
                            ),
                            &code,
                            1,
                            &image,
                            &[Step {
                                cpu,
                                ram: &[],
                                exit: Exit::Dispatch(cpu.eip),
                            }],
                        );
                    }
                }
            }
        }
    }
}

struct Expected {
    value: u32,
    carry: u8,
}

impl Expected {
    fn apply_flags(&self, cpu: &mut CpuState, prior: Flags<u8>) {
        cpu.flags.status_source.kind = 0;
        cpu.flags.bytes = FlagBytes {
            cf: self.carry,
            pf: prior.pf,
            af: prior.af,
            zf: prior.zf,
            sf: prior.sf,
            of: prior.of,
            ..cpu.flags.bytes
        };
    }
}

// Determine the old bit by division, then add or subtract its place value.
// This oracle does not reuse the generated bit masks and Boolean operations.
fn expected(operation: Operation, bits: u32, value: u32, index: u32) -> Expected {
    let value = u64::from(value) % 2_u64.pow(bits);
    let place = 2_u64.pow(index % bits);
    let carry = value / place % 2;
    let result = match operation {
        Operation::Bt => value,
        Operation::Bts => value + (1 - carry) * place,
        Operation::Btr => value - carry * place,
        Operation::Btc if carry == 0 => value + place,
        Operation::Btc => value - place,
    };
    Expected {
        value: result as u32,
        carry: carry as u8,
    }
}

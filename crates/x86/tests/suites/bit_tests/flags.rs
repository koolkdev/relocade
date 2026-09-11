use wasm86_x86::StatusFlags;

use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{expected, image, OPERATIONS};

#[test]
fn bit_tests_preserve_five_logical_flags_from_every_incoming_record_kind() {
    struct Source {
        kind: u8,
        left: u32,
        right: u32,
        flags: StatusFlags,
    }
    for source in [
        Source {
            kind: 0,
            left: 0x1234_5678,
            right: 0x8765_4321,
            flags: StatusFlags {
                cf: 0,
                pf: 0,
                af: 1,
                zf: 1,
                sf: 0,
                of: 1,
            },
        },
        Source {
            kind: 1,
            left: 0x1234_5600,
            right: 0x8765_4301,
            flags: StatusFlags {
                cf: 1,
                pf: 1,
                af: 1,
                zf: 0,
                sf: 1,
                of: 0,
            },
        },
        Source {
            kind: 5,
            left: 0x1234_8000,
            right: 0x8765_0001,
            flags: StatusFlags {
                cf: 0,
                pf: 1,
                af: 1,
                zf: 0,
                sf: 0,
                of: 1,
            },
        },
        Source {
            kind: 9,
            left: 0,
            right: 1,
            flags: StatusFlags {
                cf: 1,
                pf: 1,
                af: 1,
                zf: 0,
                sf: 1,
                of: 0,
            },
        },
        Source {
            kind: 2,
            left: 0x1234_56ff,
            right: 0x8765_4301,
            flags: StatusFlags {
                cf: 1,
                pf: 1,
                af: 1,
                zf: 1,
                sf: 0,
                of: 0,
            },
        },
        Source {
            kind: 6,
            left: 0x1234_7fff,
            right: 0x8765_0001,
            flags: StatusFlags {
                cf: 0,
                pf: 1,
                af: 1,
                zf: 0,
                sf: 1,
                of: 1,
            },
        },
        Source {
            kind: 10,
            left: u32::MAX,
            right: 1,
            flags: StatusFlags {
                cf: 1,
                pf: 1,
                af: 1,
                zf: 1,
                sf: 0,
                of: 0,
            },
        },
        Source {
            kind: 3,
            left: 0x1234_5680,
            right: 0x8765_4321,
            flags: StatusFlags {
                cf: 0,
                pf: 0,
                af: 0,
                zf: 0,
                sf: 1,
                of: 0,
            },
        },
        Source {
            kind: 7,
            left: 0x1234_0000,
            right: 0x8765_4321,
            flags: StatusFlags {
                cf: 0,
                pf: 1,
                af: 0,
                zf: 1,
                sf: 0,
                of: 0,
            },
        },
        Source {
            kind: 11,
            left: 3,
            right: 0x8765_4321,
            flags: StatusFlags {
                cf: 0,
                pf: 1,
                af: 0,
                zf: 0,
                sf: 0,
                of: 0,
            },
        },
    ] {
        for operation in OPERATIONS {
            for bits in [16, 32] {
                for index in [0, 1] {
                    let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                    code.extend_from_slice(&[0x0f, operation.register_opcode(), 0xd0]);
                    let mut image = image(&code);
                    image.cpu.flags.kind = source.kind;
                    image.cpu.flags.left = source.left;
                    image.cpu.flags.right = source.right;
                    image.cpu.registers.eax = 0x4433_8001;
                    image.cpu.registers.edx = 0x8877_0000 | index;
                    let result = expected(operation, bits, image.cpu.registers.eax, index);
                    let mut cpu = image.cpu;
                    cpu.registers.eax = if bits == 16 {
                        0x4433_0000 | result.value
                    } else {
                        result.value
                    };
                    result.apply_flags(&mut cpu, source.flags);
                    cpu.eip += code.len() as u32;
                    cpu.instruction_count = 0;
                    both(
                        TestModule::interpreter(),
                        &format!(
                            "{operation:?} {bits}-bit index {index} preserves flag kind {}",
                            source.kind
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

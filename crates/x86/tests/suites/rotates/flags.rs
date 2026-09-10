use wasm86_x86::StatusFlags;

use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{expected, image, Operation, OPERATIONS, PRIOR_FLAGS};

#[test]
fn rotates_preserve_logical_status_from_concrete_and_lazy_records() {
    struct Source {
        name: &'static str,
        kind: u8,
        left: u32,
        right: u32,
        logical: StatusFlags,
    }
    for source in [
        Source {
            name: "noncanonical concrete bytes",
            kind: 0,
            left: 0x1234_5678,
            right: 0x8765_4321,
            logical: StatusFlags {
                cf: 0,
                pf: 0,
                af: 1,
                zf: 1,
                sf: 0,
                of: 1,
            },
        },
        Source {
            name: "byte ADD zero result",
            kind: 2,
            left: 0xff,
            right: 1,
            logical: StatusFlags {
                cf: 1,
                pf: 1,
                af: 1,
                zf: 1,
                sf: 0,
                of: 0,
            },
        },
        Source {
            name: "dword ADD zero result",
            kind: 10,
            left: 0xffff_ffff,
            right: 1,
            logical: StatusFlags {
                cf: 1,
                pf: 1,
                af: 1,
                zf: 1,
                sf: 0,
                of: 0,
            },
        },
        Source {
            name: "dword SUB negative result",
            kind: 9,
            left: 0x7fff_fffe,
            right: 0xffff_fffe,
            logical: PRIOR_FLAGS,
        },
        Source {
            name: "logical result with odd parity",
            kind: 11,
            left: 1,
            right: 0x8765_4321,
            logical: StatusFlags {
                cf: 0,
                pf: 0,
                af: 0,
                zf: 0,
                sf: 0,
                of: 0,
            },
        },
    ] {
        for operation in OPERATIONS {
            for count in [0, 1, 8, 9, 32, 33] {
                for from_cl in [false, true] {
                    let mut code = vec![
                        if from_cl { 0xd2 } else { 0xc0 },
                        0xc0 | (operation.extension() << 3),
                    ];
                    if !from_cl {
                        code.push(count);
                    }
                    let mut image = image(&code);
                    image.cpu.flags.kind = source.kind;
                    image.cpu.flags.left = source.left;
                    image.cpu.flags.right = source.right;
                    image.cpu.flags.status = StatusFlags {
                        cf: 0xfe,
                        pf: 0xfe,
                        af: 0xff,
                        zf: 0xff,
                        sf: 0x80,
                        of: 0x7f,
                    };
                    image.cpu.registers.ecx = 0x8877_6600 | u32::from(count);
                    let input = if operation == Operation::Rol { 0x80 } else { 1 };
                    image.cpu.registers.eax = 0x4433_2200 | input;
                    let result = expected(operation, 8, input, count, source.logical);
                    let mut cpu = image.cpu;
                    cpu.registers.eax = 0x4433_2200 | result.value;
                    result.apply_flags(&mut cpu);
                    cpu.eip += code.len() as u32;
                    cpu.instruction_count = 0;
                    both(
                        TestModule::interpreter(),
                        &format!(
                            "{operation:?} from {}, count {count}, CL {from_cl}",
                            source.name
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

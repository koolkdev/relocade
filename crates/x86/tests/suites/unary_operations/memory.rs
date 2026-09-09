use wasm86_x86::{compile_block_from_bytes, StatusFlags};

use crate::support::{
    machine::{self, both, Exit, Image, Step},
    step::TestModule,
};

#[derive(Clone, Copy, Debug)]
enum Operation {
    Increment,
    Decrement,
    Not,
    Negate,
}

#[test]
fn unary_memory_updates_cover_each_width_and_split_mapping() {
    struct Case {
        operation: Operation,
        code: [&'static [u8]; 3],
        input: [u32; 3],
        result: [u32; 3],
    }
    for case in [
        Case {
            operation: Operation::Increment,
            code: [&[0xfe, 0x03], &[0x66, 0xff, 0x03], &[0xff, 0x03]],
            input: [0xff, 0xffff, 0xffff_ffff],
            result: [0; 3],
        },
        Case {
            operation: Operation::Decrement,
            code: [&[0xfe, 0x0b], &[0x66, 0xff, 0x0b], &[0xff, 0x0b]],
            input: [0; 3],
            result: [0xff, 0xffff, 0xffff_ffff],
        },
        Case {
            operation: Operation::Not,
            code: [&[0xf6, 0x13], &[0x66, 0xf7, 0x13], &[0xf7, 0x13]],
            input: [0x0f, 0x0f0f, 0x0f0f_0f0f],
            result: [0xf0, 0xf0f0, 0xf0f0_f0f0],
        },
        Case {
            operation: Operation::Negate,
            code: [&[0xf6, 0x1b], &[0x66, 0xf7, 0x1b], &[0xf7, 0x1b]],
            input: [1; 3],
            result: [0xff, 0xffff, 0xffff_ffff],
        },
    ] {
        for (width, code) in case.code.into_iter().enumerate() {
            let length = [1, 2, 4][width];
            for next_frame in [0x9000, 0xa000] {
                let mut image = Image::new(code);
                image.cpu.registers.ebx = 0x4fff;
                image.cpu.flags.kind = match case.operation {
                    Operation::Increment | Operation::Decrement => 2,
                    _ => 0xff,
                };
                image.cpu.flags.left = 0xff;
                image.cpu.flags.right = 1;
                image.cpu.flags.status.cf = 0;
                image.map(4, 0x8000, true);
                image.map(5, next_frame, true);
                let before = case.input[width].to_le_bytes();
                image.data(0x8ffe, &[0xa5, before[0]]);
                image.data(next_frame, &before[1..length]);
                image.data(next_frame + length as u32 - 1, &[0x5a]);
                let mut expected_cpu = image.cpu;
                match case.operation {
                    Operation::Increment => {
                        expected_cpu.flags.kind = 0;
                        expected_cpu.flags.status = StatusFlags {
                            cf: 1,
                            pf: 1,
                            af: 1,
                            zf: 1,
                            sf: 0,
                            of: 0,
                        };
                    }
                    Operation::Decrement => {
                        expected_cpu.flags.kind = 0;
                        expected_cpu.flags.status = StatusFlags {
                            cf: 1,
                            pf: 1,
                            af: 1,
                            zf: 0,
                            sf: 1,
                            of: 0,
                        };
                    }
                    Operation::Not => {}
                    Operation::Negate => {
                        expected_cpu.flags.kind = [1, 5, 9][width];
                        expected_cpu.flags.left = 0;
                        expected_cpu.flags.right = 1;
                    }
                }
                expected_cpu.eip = 0x1000 + code.len() as u32;
                expected_cpu.instruction_count = 0;
                let after = case.result[width].to_le_bytes();
                both(
                    TestModule::interpreter(),
                    &format!(
                        "{:?}, width {length}, frame {next_frame:#x}",
                        case.operation
                    ),
                    code,
                    1,
                    &image,
                    &[Step {
                        cpu: expected_cpu,
                        ram: &[(0x8fff, &after[..1]), (next_frame, &after[1..length])],
                        exit: Exit::Dispatch(expected_cpu.eip),
                    }],
                );
            }
        }
    }
}

#[test]
fn unary_rmw_proves_write_access_before_reading_flags_or_changing_memory() {
    struct Fault {
        first_writable: Option<bool>,
        next_writable: Option<bool>,
        address: u32,
        error: u16,
    }
    for code in [
        &[0xfe, 0x03][..],
        &[0xfe, 0x0b],
        &[0xf6, 0x13],
        &[0xf6, 0x1b],
        &[0x66, 0xff, 0x03],
        &[0x66, 0xff, 0x0b],
        &[0x66, 0xf7, 0x13],
        &[0x66, 0xf7, 0x1b],
        &[0xff, 0x03],
        &[0xff, 0x0b],
        &[0xf7, 0x13],
        &[0xf7, 0x1b],
    ] {
        for fault in [
            Fault {
                first_writable: None,
                next_writable: None,
                address: 0x4fff,
                error: 2,
            },
            Fault {
                first_writable: Some(false),
                next_writable: None,
                address: 0x4fff,
                error: 3,
            },
            Fault {
                first_writable: Some(true),
                next_writable: None,
                address: 0x5000,
                error: 2,
            },
            Fault {
                first_writable: Some(true),
                next_writable: Some(false),
                address: 0x5000,
                error: 3,
            },
        ] {
            if matches!(code[0], 0xfe | 0xf6) && fault.address == 0x5000 {
                continue;
            }
            let mut image = Image::new(code);
            image.cpu.registers.ebx = 0x4fff;
            // INC/DEC would trap when querying this record. The operand fault wins.
            image.cpu.flags.kind = 0xff;
            if let Some(writable) = fault.first_writable {
                image.map(4, 0x8000, writable);
            }
            if let Some(writable) = fault.next_writable {
                image.map(5, 0xa000, writable);
            }
            image.data(0x8ffe, &[0xa5, 0xff]);
            image.data(0xa000, &[0xff, 0xff, 0xff, 0x5a]);
            both(
                TestModule::interpreter(),
                "unary RMW access proof precedes effects",
                code,
                1,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: fault.address,
                        error: fault.error,
                    },
                }],
            );
        }
    }
}

#[test]
fn unary_rmw_rejects_a_wrapping_range_even_when_both_pages_are_mapped() {
    for code in [
        &[0x66, 0xff, 0x03][..],
        &[0x66, 0xff, 0x0b],
        &[0x66, 0xf7, 0x13],
        &[0x66, 0xf7, 0x1b],
        &[0xff, 0x03],
        &[0xff, 0x0b],
        &[0xf7, 0x13],
        &[0xf7, 0x1b],
    ] {
        let address = if code[0] == 0x66 {
            0xffff_ffff
        } else {
            0xffff_fffe
        };
        let mut image = Image::new(code);
        image.cpu.registers.ebx = address;
        image.cpu.flags.kind = 0xff;
        image.map(0xfffff, 0x8000, true);
        image.map(0, 0xa000, true);
        image.data(0x8ffe, &[0x11, 0x22]);
        image.data(0xa000, &[0x33, 0x44]);
        both(
            TestModule::interpreter(),
            "unary range wrap follows the memory policy",
            code,
            1,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::PageFault { address, error: 2 },
            }],
        );
    }
}

#[test]
fn completed_unary_memory_effects_survive_a_later_operand_fault() {
    let code = [
        0xff, 0x03, // INC dword [EBX]
        0xff, 0x09, // DEC dword [ECX]
    ];
    let mut image = Image::new(&code);
    image.cpu.registers.ebx = 0x4000;
    image.cpu.registers.ecx = 0x6000;
    image.cpu.flags.kind = 2;
    image.cpu.flags.left = 0xff;
    image.cpu.flags.right = 1;
    image.cpu.flags.status.cf = 0;
    image.map(4, 0x8000, true);
    image.data(0x7fff, &[0xa5, 0xff, 0xff, 0xff, 0xff, 0x5a]);
    let mut expected_cpu = image.cpu;
    let mut steps = Vec::new();

    expected_cpu.flags.kind = 0;
    expected_cpu.flags.status = StatusFlags {
        cf: 1,
        pf: 1,
        af: 1,
        zf: 1,
        sf: 0,
        of: 0,
    };
    expected_cpu.eip = 0x1002;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[(0x8000, &[0, 0, 0, 0])],
        exit: Exit::Dispatch(0x1002),
    });

    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::PageFault {
            address: 0x6000,
            error: 2,
        },
    });

    both(
        TestModule::interpreter(),
        "completed unary memory store precedes later fault",
        &code,
        2,
        &image,
        &steps,
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn mixed_width_unary_memory_effects_and_carry_survive_a_fault_in_optimizing_v8() {
    let code = [
        0xfe, 0x03, // INC byte [EBX]
        0x66, 0xff, 0x0b, // DEC word [EBX]
        0xff, 0x01, // INC dword [ECX], whose page is absent.
    ];
    let mut image = Image::new(&code);
    image.cpu.registers.ebx = 0x4fff;
    image.cpu.registers.ecx = 0x6000;
    image.cpu.flags.kind = 2;
    image.cpu.flags.left = 0xff;
    image.cpu.flags.right = 1;
    image.cpu.flags.status.cf = 0;
    image.map(4, 0x8000, true);
    image.map(5, 0xa000, true);
    image.data(0x8ffe, &[0xa5, 0xff]);
    image.data(0xa000, &[0, 0x5a]);
    let mut expected_cpu = image.cpu;
    let mut steps = Vec::new();

    expected_cpu.flags.kind = 0;
    expected_cpu.flags.status = StatusFlags {
        cf: 1,
        pf: 1,
        af: 1,
        zf: 1,
        sf: 0,
        of: 0,
    };
    expected_cpu.eip = 0x1002;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[(0x8fff, &[0])],
        exit: Exit::Dispatch(0x1002),
    });

    expected_cpu.flags.status = StatusFlags {
        cf: 1,
        pf: 1,
        af: 1,
        zf: 0,
        sf: 1,
        of: 0,
    };
    expected_cpu.eip = 0x1005;
    expected_cpu.instruction_count = 1;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[(0x8fff, &[0xff]), (0xa000, &[0xff])],
        exit: Exit::Dispatch(0x1005),
    });

    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::PageFault {
            address: 0x6000,
            error: 2,
        },
    });

    assert_eq!(
        TestModule::interpreter().observe_v8(&image.input(), 3),
        machine::expected(&image, &steps),
    );
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 3).unwrap());
    assert_eq!(
        block.observe_v8(&image.input(), 1),
        machine::expected(
            &image,
            &[Step {
                cpu: expected_cpu,
                ram: &[(0x8fff, &[0xff]), (0xa000, &[0xff])],
                exit: Exit::PageFault {
                    address: 0x6000,
                    error: 2
                },
            }]
        ),
    );
}

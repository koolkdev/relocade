use wasm86_x86::StatusFlags;

use crate::support::{
    conditions::check_conditions,
    guest::{Exit, Machine},
    machine::Image,
    step::TestModule,
};

#[path = "unary_operations/carry.rs"]
mod carry;
#[path = "unary_operations/decoding.rs"]
mod decoding;
#[path = "unary_operations/memory.rs"]
mod memory;
#[path = "unary_operations/registers.rs"]
mod registers;

struct Boundary {
    input: [u32; 3],
    result: [u32; 3],
    parity: [u8; 3],
    auxiliary: u8,
    zero: u8,
    sign: u8,
    overflow: u8,
}

#[test]
fn increment_and_decrement_boundaries_preserve_carry() {
    // Each row gives independent byte, word, and dword expectations. Parity
    // always uses the low byte, including at the word and dword sign boundaries.
    let increments = [
        Boundary {
            input: [0; 3],
            result: [1; 3],
            parity: [0; 3],
            auxiliary: 0,
            zero: 0,
            sign: 0,
            overflow: 0,
        },
        Boundary {
            input: [15; 3],
            result: [16; 3],
            parity: [0; 3],
            auxiliary: 1,
            zero: 0,
            sign: 0,
            overflow: 0,
        },
        Boundary {
            input: [0x7f, 0x7fff, 0x7fff_ffff],
            result: [0x80, 0x8000, 0x8000_0000],
            parity: [0, 1, 1],
            auxiliary: 1,
            zero: 0,
            sign: 1,
            overflow: 1,
        },
        Boundary {
            input: [0xff, 0xffff, 0xffff_ffff],
            result: [0; 3],
            parity: [1; 3],
            auxiliary: 1,
            zero: 1,
            sign: 0,
            overflow: 0,
        },
    ];
    let decrements = [
        Boundary {
            input: [1; 3],
            result: [0; 3],
            parity: [1; 3],
            auxiliary: 0,
            zero: 1,
            sign: 0,
            overflow: 0,
        },
        Boundary {
            input: [16; 3],
            result: [15; 3],
            parity: [1; 3],
            auxiliary: 1,
            zero: 0,
            sign: 0,
            overflow: 0,
        },
        Boundary {
            input: [0x80, 0x8000, 0x8000_0000],
            result: [0x7f, 0x7fff, 0x7fff_ffff],
            parity: [0, 1, 1],
            auxiliary: 1,
            zero: 0,
            sign: 0,
            overflow: 1,
        },
        Boundary {
            input: [0; 3],
            result: [0xff, 0xffff, 0xffff_ffff],
            parity: [1; 3],
            auxiliary: 1,
            zero: 0,
            sign: 1,
            overflow: 0,
        },
    ];
    for (encodings, boundaries) in [
        (
            [&[0xfe, 0xc0][..], &[0x66, 0xff, 0xc0], &[0xff, 0xc0]],
            &increments,
        ),
        (
            [&[0xfe, 0xc8][..], &[0x66, 0xff, 0xc8], &[0xff, 0xc8]],
            &decrements,
        ),
    ] {
        for (width, code) in encodings.into_iter().enumerate() {
            for boundary in boundaries {
                for carry in [0, 1] {
                    let mut machine = Machine::new(code);
                    machine.cpu.flags.kind = 0;
                    machine.cpu.flags.status.cf = carry;
                    let upper = [0x1234_5600, 0x1234_0000, 0][width];
                    machine.cpu.registers.eax = upper | boundary.input[width];
                    let mut expected = machine.state();
                    expected.cpu.registers.eax = upper | boundary.result[width];
                    expected.cpu.flags.status = StatusFlags {
                        cf: carry,
                        pf: boundary.parity[width],
                        af: boundary.auxiliary,
                        zf: boundary.zero,
                        sf: boundary.sign,
                        of: boundary.overflow,
                    };
                    expected.cpu.eip = 0x1000 + code.len() as u32;
                    expected.cpu.instruction_count = 0;

                    for execution in [machine.run_step(), machine.run_block(1)] {
                        assert_eq!(execution.exit, Exit::Dispatch(expected.cpu.eip));
                        assert_eq!(
                            execution.state, expected,
                            "{code:02x?}, input {:#x}, carry {carry}",
                            boundary.input[width]
                        );
                        assert_eq!(execution.dispatches, [(expected.cpu.eip, expected.clone())]);
                        assert!(execution.machine_unchanged);
                    }
                }
            }
        }
    }
}

#[test]
fn negation_replaces_invalid_flags_and_reports_boundary_conditions() {
    struct Negation {
        input: [u32; 3],
        result: [u32; 3],
        // Bit n is the expected SETcc result for condition n, O through G.
        conditions: [u16; 3],
    }
    for boundary in [
        Negation {
            input: [0; 3],
            result: [0; 3],
            conditions: [0x665a; 3],
        },
        Negation {
            input: [1; 3],
            result: [0xff, 0xffff, 0xffff_ffff],
            conditions: [0x5566; 3],
        },
        Negation {
            input: [0x10; 3],
            result: [0xf0, 0xfff0, 0xffff_fff0],
            conditions: [0x5566; 3],
        },
        Negation {
            input: [0x7f, 0x7fff, 0x7fff_ffff],
            result: [0x81, 0x8001, 0x8000_0001],
            conditions: [0x5566, 0x5966, 0x5966],
        },
        Negation {
            input: [0x80, 0x8000, 0x8000_0000],
            result: [0x80, 0x8000, 0x8000_0000],
            conditions: [0xa965, 0xa565, 0xa565],
        },
        Negation {
            input: [0xff, 0xffff, 0xffff_ffff],
            result: [1; 3],
            conditions: [0xaa66; 3],
        },
    ] {
        for (width, code) in [&[0xf6, 0xd8][..], &[0x66, 0xf7, 0xd8], &[0xf7, 0xd8]]
            .into_iter()
            .enumerate()
        {
            let mut image = Image::new(code);
            image.cpu.flags.kind = 0xff;
            let upper = [0x1234_5600, 0x1234_0000, 0][width];
            image.cpu.registers.eax = upper | boundary.input[width];
            let mut expected_cpu = image.cpu;
            expected_cpu.registers.eax = upper | boundary.result[width];
            expected_cpu.flags.kind = [1, 5, 9][width];
            expected_cpu.flags.left = 0;
            expected_cpu.flags.right = boundary.input[width];
            check_conditions(
                TestModule::interpreter(),
                &format!("NEG {code:02x?}, input {:#x}", boundary.input[width]),
                code,
                &mut image,
                &expected_cpu,
                boundary.conditions[width],
            );
        }
    }
}

#[test]
fn not_preserves_every_flag_byte_without_reading_the_record() {
    for (code, input, result) in [
        (&[0xf6, 0xd0][..], 0x1234_5600, 0x1234_56ff),
        (&[0x66, 0xf7, 0xd0][..], 0x1234_0000, 0x1234_ffff),
        (&[0xf7, 0xd0][..], 0x1234_5678, 0xedcb_a987),
    ] {
        for kind in [0, 2, 7, 9, 0xff] {
            let mut machine = Machine::new(code);
            machine.cpu.flags.kind = kind;
            machine.cpu.flags.left = 0x0123_4567;
            machine.cpu.flags.right = 0x89ab_cdef;
            machine.cpu.flags.status = StatusFlags {
                cf: 0x80,
                pf: 0x81,
                af: 0xfe,
                zf: 0xff,
                sf: 0x55,
                of: 0xaa,
            };
            machine.cpu.registers.eax = input;
            let mut expected = machine.state();
            expected.cpu.registers.eax = result;
            expected.cpu.eip = 0x1000 + code.len() as u32;
            expected.cpu.instruction_count = 0;
            for execution in [machine.run_step(), machine.run_block(1)] {
                assert_eq!(execution.exit, Exit::Dispatch(expected.cpu.eip));
                assert_eq!(execution.state, expected, "{code:02x?}, flag kind {kind}");
                assert_eq!(execution.dispatches, [(expected.cpu.eip, expected.clone())]);
                assert!(execution.machine_unchanged);
            }
        }
    }
}

#[test]
fn setcc_combines_preserved_carry_with_the_new_increment_or_decrement_result() {
    for (code, input, result, zero, sign, conditions) in [
        (&[0xfe, 0xc0][..], 0xff, 0, 1, 0, [0x665a, 0x6656]),
        (&[0xfe, 0xc8][..], 0, 0xff, 0, 1, [0x55aa, 0x5566]),
    ] {
        for carry in [0, 1] {
            let mut image = Image::new(code);
            image.cpu.flags.kind = 0;
            image.cpu.flags.status.cf = carry;
            image.cpu.registers.eax = 0x1234_5600 | input;
            let mut expected_cpu = image.cpu;
            expected_cpu.registers.eax = 0x1234_5600 | result;
            expected_cpu.flags.status = StatusFlags {
                cf: carry,
                pf: 1,
                af: 1,
                zf: zero,
                sf: sign,
                of: 0,
            };
            check_conditions(
                TestModule::interpreter(),
                "SETcc uses both the preserved carry and the new arithmetic flags",
                code,
                &mut image,
                &expected_cpu,
                conditions[usize::from(carry)],
            );
        }
    }
}

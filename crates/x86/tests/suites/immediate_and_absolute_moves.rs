use wasm86_x86::{compile_block_from_bytes, BlockError, Gpr32};
use wasmparser::Validator;

use crate::support::machine;
use crate::support::step;
use machine::{both, check, Exit, Step};
use step::TestModule;

use machine::byte_register_image as image;

#[test]
fn selected_form_lengths_include_the_address_then_the_immediate() {
    for code in [
        &[0xc6, 0xc4, 0x80][..],
        &[0xc7, 0xc0, 0xa0, 0x66, 0xc7, 0x88][..],
        &[0xc6, 0x44, 0x8b, 0x7f, 0xff][..],
        &[0xc7, 0x84, 0x8b, 0, 0x40, 0, 0, 0xa0, 0x66, 0xc7, 0x88][..],
        &[0xa0, 0x20, 0x40, 0, 0x80][..],
        &[0xa1, 0x20, 0x40, 0, 0x80][..],
        &[0xa2, 0x20, 0x40, 0, 0x80][..],
        &[0xa3, 0x20, 0x40, 0, 0x80][..],
    ] {
        for available in 0..code.len() {
            assert!(matches!(
                compile_block_from_bytes(0x1000, &code[..available], 1),
                Err(BlockError::TruncatedInstruction { address: 0x1000, available: actual })
                    if actual == available
            ));
        }
        let module = compile_block_from_bytes(0x1000, code, 1).unwrap();
        Validator::new().validate_all(&module.bytes).unwrap();
        let mut with_suffix = code.to_vec();
        with_suffix.extend_from_slice(&[0xc7, 0x0c]);
        assert_eq!(
            compile_block_from_bytes(0x1000, &with_suffix, 1)
                .unwrap()
                .bytes,
            module.bytes
        );
    }
}

#[test]
fn unsupported_group_extensions_do_not_require_an_address_or_immediate() {
    // These ModRM bytes would require a SIB or disp32 if the group matched.
    for (opcode, modrm) in [(0xc6, 0x0c), (0xc7, 0x3d)] {
        for suffix in [&[][..], &[0x24, 0, 0, 0, 0, 0x80][..]] {
            let mut code = vec![opcode, modrm];
            code.extend_from_slice(suffix);
            assert!(matches!(
                compile_block_from_bytes(0x1ffe, &code, 1),
                Err(BlockError::UnsupportedInstruction { address: 0x1ffe, opcode: actual })
                    if actual == opcode
            ));
        }
    }
}

#[test]
fn register_immediates() {
    use crate::support::guest::{Exit as GuestExit, Machine};
    use Gpr32::*;
    // The parent words are literal byte-alias expectations; ModRM's reg field
    // selects the /0 form, while its r/m field selects the destination.
    for (register, name, parent, immediate, result) in [
        (0, "AL", Eax, 0x80, 0x4433_2280),
        (1, "CL", Ecx, 0, 0x8877_6600),
        (2, "DL", Edx, 0xff, 0xccbb_aaff),
        (3, "BL", Ebx, 0x66, 0x10ff_ee66),
        (4, "AH", Eax, 0xc6, 0x4433_c611),
        (5, "CH", Ecx, 0xa0, 0x8877_a055),
        (6, "DH", Edx, 0xc7, 0xccbb_c799),
        (7, "BH", Ebx, 0x7f, 0x10ff_7fdd),
    ] {
        let code = [0xc6, 0xc0 + register, immediate];
        let mut machine = Machine::new(&code);
        machine.cpu = image(&[]).cpu;
        let mut expected = machine.state();
        expected.cpu.registers[parent] = result;
        expected.cpu.eip = 0x1003;
        expected.cpu.instruction_count = 0;
        for actual in [machine.run_step(), machine.run_block(1)] {
            assert_eq!(
                actual.exit,
                GuestExit::Dispatch(0x1003),
                "C6 immediate to {name}"
            );
            assert_eq!(
                actual.dispatches,
                [(0x1003, expected.clone())],
                "C6 immediate to {name}"
            );
            assert_eq!(actual.state, expected, "C6 immediate to {name}");
            assert!(actual.machine_unchanged);
        }
    }
    for (register, name, parent) in [
        (0, "EAX", Eax),
        (1, "ECX", Ecx),
        (2, "EDX", Edx),
        (3, "EBX", Ebx),
        (4, "ESP", Esp),
        (5, "EBP", Ebp),
        (6, "ESI", Esi),
        (7, "EDI", Edi),
    ] {
        let code = [0xc7, 0xc0 + register, 0xa0, 0x66, 0xc7, 0x88];
        let mut machine = Machine::new(&code);
        machine.cpu = image(&[]).cpu;
        let mut expected = machine.state();
        expected.cpu.registers[parent] = 0x88c7_66a0;
        expected.cpu.eip = 0x1006;
        expected.cpu.instruction_count = 0;
        for actual in [machine.run_step(), machine.run_block(1)] {
            assert_eq!(
                actual.exit,
                GuestExit::Dispatch(0x1006),
                "C7 immediate to {name}"
            );
            assert_eq!(
                actual.dispatches,
                [(0x1006, expected.clone())],
                "C7 immediate to {name}"
            );
            assert_eq!(actual.state, expected, "C7 immediate to {name}");
            assert!(actual.machine_unchanged);
        }
    }
}

#[test]
fn addressed_immediates() {
    let step = TestModule::interpreter();
    for (name, code, registers, address, stored) in [
        (
            "byte through base",
            &[0xc6, 0x03, 0x80][..],
            &[(Gpr32::Ebx, 0x4020)][..],
            0x8020,
            &[0x80][..],
        ),
        (
            "dword after negative disp8",
            &[0xc7, 0x43, 0x80, 0xa0, 0x66, 0xc7, 0x88][..],
            &[(Gpr32::Ebx, 0x4080)][..],
            0x8000,
            &[0xa0, 0x66, 0xc7, 0x88][..],
        ),
        (
            "byte after scaled index and positive disp8",
            &[0xc6, 0x44, 0x8b, 0x7f, 0xff][..],
            &[(Gpr32::Ebx, 0x3f01), (Gpr32::Ecx, 32)][..],
            0x8000,
            &[0xff][..],
        ),
        (
            "dword after wrapped scaled address",
            &[0xc7, 0x84, 0x8b, 0, 0x40, 0, 0, 0xa0, 0x66, 0xc7, 0x88][..],
            &[(Gpr32::Ebx, 0xffff_fff0), (Gpr32::Ecx, 4)][..],
            0x8000,
            &[0xa0, 0x66, 0xc7, 0x88][..],
        ),
        (
            "byte with neither base nor index",
            &[0xc6, 0x04, 0x25, 0x20, 0x40, 0, 0, 0xb7][..],
            &[][..],
            0x8020,
            &[0xb7][..],
        ),
        (
            "dword with index and no base",
            &[0xc7, 0x04, 0x8d, 0xf0, 0x3f, 0, 0, 0xff, 0xff, 0xff, 0xff][..],
            &[(Gpr32::Ecx, 4)][..],
            0x8000,
            &[0xff, 0xff, 0xff, 0xff][..],
        ),
    ] {
        let mut image = image(code);
        for &(register, value) in registers {
            image.cpu.registers[register] = value;
        }
        image.map(4, 0x8000, true);
        let mut before = vec![0xa5];
        before.extend(vec![0xcc; stored.len()]);
        before.push(0x5a);
        image.data(address - 1, &before);
        let next_eip = 0x1000 + code.len() as u32;
        let mut expected_cpu = image.cpu;
        expected_cpu.eip = next_eip;
        expected_cpu.instruction_count = 0;
        both(
            step,
            name,
            code,
            1,
            &image,
            &[Step {
                cpu: expected_cpu,
                ram: &[(address, stored)],
                exit: Exit::Dispatch(next_eip),
            }],
        );
    }
}

#[test]
fn absolute_offsets() {
    let step = TestModule::interpreter();
    for (opcode, eax, ram) in [
        (0xa0, Some(0x4433_22a0), &[][..]),
        (0xa1, Some(0x88c7_66a0), &[][..]),
        (0xa2, None, &[(0x8020, &[0x11][..])][..]),
        (0xa3, None, &[(0x8020, &[0x11, 0x22, 0x33, 0x44][..])][..]),
    ] {
        // Both byte and dword data forms have a full 32-bit absolute offset.
        let code = [opcode, 0x20, 0x40, 0, 0x80];
        let mut image = image(&code);
        image.map(0x80004, 0x8000, true);
        image.data(0x801f, &[0xa5, 0xa0, 0x66, 0xc7, 0x88, 0x5a]);
        let mut expected_cpu = image.cpu;
        if let Some(value) = eax {
            expected_cpu.registers.eax = value;
        }
        expected_cpu.eip = 0x1005;
        expected_cpu.instruction_count = 0;
        both(
            step,
            &format!("absolute opcode {opcode:02x} with high offset bit"),
            &code,
            1,
            &image,
            &[Step {
                cpu: expected_cpu,
                ram,
                exit: Exit::Dispatch(0x1005),
            }],
        );
    }
    for (opcode, eax, ram) in [
        (0xa0, Some(0x4433_2280), &[][..]),
        (0xa2, None, &[(0x8fff, &[0x11][..])][..]),
    ] {
        let code = [opcode, 0xff, 0xff, 0xff, 0xff];
        let mut image = image(&code);
        image.map(0xfffff, 0x8000, true);
        image.data(0x8ffe, &[0xa5, 0x80]);
        let mut expected_cpu = image.cpu;
        if let Some(value) = eax {
            expected_cpu.registers.eax = value;
        }
        expected_cpu.eip = 0x1005;
        expected_cpu.instruction_count = 0;
        both(
            step,
            "absolute byte at the last linear address",
            &code,
            1,
            &image,
            &[Step {
                cpu: expected_cpu,
                ram,
                exit: Exit::Dispatch(0x1005),
            }],
        );
    }
    for (name, next_frame) in [("contiguous", 0x9000), ("scattered", 0xa000)] {
        for opcode in [0xa1, 0xa3] {
            let code = [opcode, 0xfe, 0x4f, 0, 0];
            let mut image = image(&code);
            image.map(4, 0x8000, true);
            image.map(5, next_frame, true);
            image.data(0x8ffd, &[0xa5, 0xa0, 0x66]);
            image.data(next_frame, &[0xc7, 0x88, 0x5a]);
            let mut expected_cpu = image.cpu;
            expected_cpu.eip = 0x1005;
            expected_cpu.instruction_count = 0;
            let ram = if opcode == 0xa1 {
                expected_cpu.registers.eax = 0x88c7_66a0;
                vec![]
            } else {
                vec![(0x8ffe, &[0x11, 0x22][..]), (next_frame, &[0x33, 0x44][..])]
            };
            both(
                step,
                &format!("absolute dword {opcode:02x} across {name} pages"),
                &code,
                1,
                &image,
                &[Step {
                    cpu: expected_cpu,
                    ram: &ram,
                    exit: Exit::Dispatch(0x1005),
                }],
            );
        }
    }
}

#[test]
fn data_faults() {
    let step = TestModule::interpreter();
    let code = [0xa0, 0, 0x40, 0, 0];
    let mut invalid_backing = image(&code);
    invalid_backing.map(4, 0x10000, false);
    let expected_cpu = invalid_backing.cpu;
    both(
        step,
        "present frame outside RAM traps before updating AL",
        &code,
        1,
        &invalid_backing,
        &[Step {
            cpu: expected_cpu,
            ram: &[],
            exit: Exit::Trap,
        }],
    );
    let code = [0xa1, 0xfe, 0x4f, 0, 0];
    let mut missing = image(&code);
    missing.map(4, 0x8000, false);
    missing.data(0x8ffe, &[0xa0, 0x66]);
    let expected_cpu = missing.cpu;
    both(
        step,
        "absolute read reports the missing second page",
        &code,
        1,
        &missing,
        &[Step {
            cpu: expected_cpu,
            ram: &[],
            exit: Exit::PageFault {
                address: 0x00005000,
                error: 0x0,
            },
        }],
    );
    for code in [
        &[0xa3, 0xfe, 0x4f, 0, 0][..],
        &[0xc7, 0x03, 0xa0, 0x66, 0xc7, 0x88][..],
    ] {
        let mut denied = image(code);
        denied.cpu.registers.ebx = 0x4ffe;
        denied.map(4, 0x8000, true);
        denied.map(5, 0xa000, false);
        denied.data(0x8ffd, &[0xa5, 1, 2]);
        denied.data(0xa000, &[3, 4, 0x5a]);
        let expected_cpu = denied.cpu;
        both(
            step,
            "denied second page leaves the entire dword unchanged",
            code,
            1,
            &denied,
            &[Step {
                cpu: expected_cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x00005000,
                    error: 0x3,
                },
            }],
        );
    }
    for (opcode, fault) in [
        (
            0xa1,
            Exit::PageFault {
                address: 0xfffffffe,
                error: 0x0,
            },
        ),
        (
            0xa3,
            Exit::PageFault {
                address: 0xfffffffe,
                error: 0x2,
            },
        ),
    ] {
        let code = [opcode, 0xfe, 0xff, 0xff, 0xff];
        let mut wrapped = image(&code);
        wrapped.map(0xfffff, 0x8000, true);
        wrapped.map(0, 0xa000, true);
        wrapped.data(0x8ffe, &[1, 2]);
        wrapped.data(0xa000, &[3, 4]);
        let expected_cpu = wrapped.cpu;
        both(
            step,
            "absolute dword span rejects linear wrap",
            &code,
            1,
            &wrapped,
            &[Step {
                cpu: expected_cpu,
                ram: &[],
                exit: fault,
            }],
        );
    }
}

#[test]
fn progress_and_aliases() {
    let step = TestModule::interpreter();
    let code = [
        0xc7, 0xc3, 0, 0x40, 0, 0, 0xc6, 0x03, 0x80, 0xa1, 0, 0x50, 0, 0,
    ];
    for (name, exit) in [
        (
            "prior register and byte store survive a later fault",
            Exit::PageFault {
                address: 0x00005000,
                error: 0x0,
            },
        ),
        (
            "forwarded address continues through an absolute load",
            Exit::Dispatch(0x100e),
        ),
    ] {
        let mut image = image(&code);
        image.map(4, 0x8000, true);
        image.data(0x7fff, &[0xa5, 0xcc, 0x5a]);
        image.data(0x9000, &[0xa0, 0x66, 0xc7, 0x88]);
        if matches!(exit, Exit::Dispatch(_)) {
            image.map(5, 0x9000, false);
        }
        let mut expected_cpu = image.cpu;
        let mut steps = Vec::new();

        expected_cpu.registers.ebx = 0x4000;
        expected_cpu.eip = 0x1006;
        expected_cpu.instruction_count = 0;
        steps.push(Step {
            cpu: expected_cpu,
            ram: &[],
            exit: Exit::Dispatch(0x1006),
        });

        expected_cpu.eip = 0x1009;
        expected_cpu.instruction_count = 1;
        steps.push(Step {
            cpu: expected_cpu,
            ram: &[(0x8000, &[0x80])],
            exit: Exit::Dispatch(0x1009),
        });

        if matches!(exit, Exit::Dispatch(_)) {
            expected_cpu.registers.eax = 0x88c7_66a0;
            expected_cpu.eip = 0x100e;
            expected_cpu.instruction_count = 2;
        }
        steps.push(Step {
            cpu: expected_cpu,
            ram: &[],
            exit,
        });

        both(step, name, &code, 3, &image, &steps);
    }
    let code = [
        0xc6, 0xc4, 0x80, 0xa3, 0, 0x40, 0, 0, 0xc7, 0xc0, 0xef, 0xbe, 0xad, 0xde, 0xa0, 1, 0x40,
        0, 0,
    ];
    let mut aliases = image(&code);
    aliases.map(4, 0x8000, true);
    aliases.data(0x7fff, &[0xa5, 0, 0, 0, 0, 0x5a]);
    let mut expected_cpu = aliases.cpu;
    let mut steps = Vec::new();

    expected_cpu.registers.eax = 0x4433_8011;
    expected_cpu.eip = 0x1003;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1003),
    });

    expected_cpu.eip = 0x1008;
    expected_cpu.instruction_count = 1;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[(0x8000, &[0x11, 0x80, 0x33, 0x44])],
        exit: Exit::Dispatch(0x1008),
    });

    expected_cpu.registers.eax = 0xdead_beef;
    expected_cpu.eip = 0x100e;
    expected_cpu.instruction_count = 2;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x100e),
    });

    expected_cpu.registers.eax = 0xdead_be80;
    expected_cpu.eip = 0x1013;
    expected_cpu.instruction_count = 3;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1013),
    });

    both(
        step,
        "accumulator byte and dword views share their stored value",
        &code,
        4,
        &aliases,
        &steps,
    );
    let code = [
        0xa1, 0, 0x40, 0, 0, 0xc7, 0x05, 0, 0x60, 0, 0, 0x99, 0x77, 0x66, 0x55, 0x89, 0xc6,
    ];
    let mut aliases = image(&code);
    aliases.map(4, 0x8000, true);
    aliases.map(6, 0x8000, true);
    aliases.data(0x7fff, &[0xa5, 0x11, 0x22, 0x33, 0x44, 0x5a]);
    let mut expected_cpu = aliases.cpu;
    let mut steps = Vec::new();

    expected_cpu.registers.eax = 0x4433_2211;
    expected_cpu.eip = 0x1005;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1005),
    });

    expected_cpu.eip = 0x100f;
    expected_cpu.instruction_count = 1;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[(0x8000, &[0x99, 0x77, 0x66, 0x55])],
        exit: Exit::Dispatch(0x100f),
    });

    expected_cpu.registers.esi = 0x4433_2211;
    expected_cpu.eip = 0x1011;
    expected_cpu.instruction_count = 2;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1011),
    });

    both(
        step,
        "held absolute load survives a grouped store through a physical alias",
        &code,
        3,
        &aliases,
        &steps,
    );
}

#[test]
fn encoding_fetches() {
    let step = TestModule::interpreter();
    for (name, start, available) in [
        ("missing grouped ModRM", 0x1fff, &[0xc6][..]),
        ("missing grouped displacement", 0x1ffe, &[0xc7, 0x05][..]),
        (
            "missing immediate wins over a missing data page",
            0x1ff9,
            &[0xc7, 0x05, 0, 0x40, 0, 0, 0xa0][..],
        ),
        (
            "missing absolute offset wins over a missing data page",
            0x1ffc,
            &[0xa0, 0, 0x40, 0][..],
        ),
    ] {
        let mut missing = image(&[]);
        missing.cpu.eip = start;
        missing.data(0x3000 + (start & 0xfff), available);
        let expected_cpu = missing.cpu;
        check(
            step,
            name,
            &missing,
            &[Step {
                cpu: expected_cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x00002000,
                    error: 0x10,
                },
            }],
        );
    }
    for (opcode, modrm) in [(0xc6, 0x0c), (0xc7, 0x3d)] {
        let mut unsupported = image(&[]);
        unsupported.cpu.eip = 0x1ffe;
        unsupported.data(0x3ffe, &[opcode, modrm]);
        let expected_cpu = unsupported.cpu;
        check(
            step,
            "unsupported group does not fetch the inaccessible address tail",
            &unsupported,
            &[Step {
                cpu: expected_cpu,
                ram: &[],
                exit: Exit::Other((8 << 48) | ((opcode as u64) << 32) | 0x1ffe),
            }],
        );
    }
    for (name, start, code, stored) in [
        (
            "group ends at the mapped page boundary",
            0x1ffd,
            &[0xc6, 0x03, 0x80][..],
            &[0x80][..],
        ),
        (
            "absolute offset ends at the mapped page boundary",
            0x1ffb,
            &[0xa3, 0, 0x40, 0, 0][..],
            &[0x11, 0x22, 0x33, 0x44][..],
        ),
    ] {
        let mut complete = image(&[]);
        complete.cpu.registers.ebx = 0x4000;
        complete.cpu.eip = start;
        complete.map(4, 0x8000, true);
        complete.data(0x3000 + (start & 0xfff), code);
        complete.data(0x7fff, &[0xa5, 0, 0, 0, 0, 0x5a]);
        let mut expected_cpu = complete.cpu;
        expected_cpu.eip = 0x2000;
        expected_cpu.instruction_count = 0;
        both(
            step,
            name,
            code,
            1,
            &complete,
            &[Step {
                cpu: expected_cpu,
                ram: &[(0x8000, stored)],
                exit: Exit::Dispatch(0x2000),
            }],
        );
    }
    let code = [0xc7, 0x84, 0x8b, 0, 0x40, 0, 0, 0xa0, 0x66, 0xc7, 0x88];
    let mut scattered = image(&[]);
    scattered.cpu.registers.ebx = 0xffff_fff0;
    scattered.cpu.registers.ecx = 4;
    scattered.cpu.eip = 0x1ff9;
    scattered.map(2, 0xa000, false);
    scattered.map(4, 0x8000, true);
    scattered.data(0x3ff9, &code[..7]);
    scattered.data(0xa000, &code[7..]);
    scattered.data(0x7fff, &[0xa5, 0, 0, 0, 0, 0x5a]);
    let mut expected_cpu = scattered.cpu;
    expected_cpu.eip = 0x2004;
    expected_cpu.instruction_count = 0;
    both(
        step,
        "eleven-byte instruction fetch crosses scattered frames",
        &code,
        1,
        &scattered,
        &[Step {
            cpu: expected_cpu,
            ram: &[(0x8000, &[0xa0, 0x66, 0xc7, 0x88])],
            exit: Exit::Dispatch(0x2004),
        }],
    );
    let code = [0xc7, 0xc0, 0xa0, 0x66, 0xc7, 0x88];
    let mut wrapped = image(&[]);
    wrapped.cpu.eip = 0xffff_fffc;
    wrapped.map(0xfffff, 0x8000, false);
    wrapped.map(0, 0xa000, false);
    wrapped.data(0x8ffc, &code[..4]);
    wrapped.data(0xa000, &code[4..]);
    let mut expected_cpu = wrapped.cpu;
    expected_cpu.registers.eax = 0x88c7_66a0;
    expected_cpu.eip = 2;
    expected_cpu.instruction_count = 0;
    both(
        step,
        "group immediate fetch wraps EIP",
        &code,
        1,
        &wrapped,
        &[Step {
            cpu: expected_cpu,
            ram: &[],
            exit: Exit::Dispatch(2),
        }],
    );
}

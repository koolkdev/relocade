use wasm86_x86::{Gpr32, StatusFlags};

use crate::support::guest::{Exit, Machine};

const REGISTERS: [(u8, Gpr32); 8] = [
    (0, Gpr32::Eax),
    (1, Gpr32::Ecx),
    (2, Gpr32::Edx),
    (3, Gpr32::Ebx),
    (4, Gpr32::Esp),
    (5, Gpr32::Ebp),
    (6, Gpr32::Esi),
    (7, Gpr32::Edi),
];

#[test]
fn compact_increment_and_decrement_select_every_word_and_dword_register() {
    for (base, input, result, zero, sign) in
        [(0x40, 0xffff_ffff, 0, 1, 0), (0x48, 0, 0xffff_ffff, 0, 1)]
    {
        for word in [false, true] {
            for (index, register) in REGISTERS {
                let code = if word {
                    vec![0x66, base + index]
                } else {
                    vec![base + index]
                };
                let mut machine = Machine::new(&code);
                machine.cpu.flags.kind = 0;
                machine.cpu.flags.status.cf = 1;
                machine.cpu.registers[register] = if word {
                    0x9234_0000 | (input & 0xffff)
                } else {
                    input
                };
                let mut expected = machine.state();
                expected.cpu.registers[register] = if word {
                    0x9234_0000 | (result & 0xffff)
                } else {
                    result
                };
                expected.cpu.flags.status = StatusFlags {
                    cf: 1,
                    pf: 1,
                    af: 1,
                    zf: zero,
                    sf: sign,
                    of: 0,
                };
                expected.cpu.eip = 0x1000 + code.len() as u32;
                expected.cpu.instruction_count = 0;
                for execution in [machine.run_step(), machine.run_block(1)] {
                    assert_eq!(execution.exit, Exit::Dispatch(expected.cpu.eip));
                    assert_eq!(execution.state, expected, "{code:02x?}, {register:?}");
                    assert_eq!(execution.dispatches, [(expected.cpu.eip, expected.clone())]);
                    assert!(execution.machine_unchanged);
                }
            }
        }
    }
}

#[test]
fn unary_modrm_selects_every_dword_register() {
    for (opcode, extension, result) in [
        (0xff, 0x00, 2),           // INC
        (0xff, 0x08, 0),           // DEC
        (0xf7, 0x10, 0xffff_fffe), // NOT
        (0xf7, 0x18, 0xffff_ffff), // NEG
    ] {
        for (index, register) in REGISTERS {
            let code = [opcode, 0xc0 | extension | index];
            let mut machine = Machine::new(&code);
            machine.cpu.flags.kind = if opcode == 0xff { 0 } else { 0xff };
            machine.cpu.flags.status.cf = 1;
            machine.cpu.registers[register] = 1;
            let mut expected = machine.state();
            expected.cpu.registers[register] = result;
            match extension {
                0x00 => {
                    expected.cpu.flags.status = StatusFlags {
                        cf: 1,
                        pf: 0,
                        af: 0,
                        zf: 0,
                        sf: 0,
                        of: 0,
                    }
                }
                0x08 => {
                    expected.cpu.flags.status = StatusFlags {
                        cf: 1,
                        pf: 1,
                        af: 0,
                        zf: 1,
                        sf: 0,
                        of: 0,
                    }
                }
                0x10 => {}
                0x18 => {
                    expected.cpu.flags.kind = 9;
                    expected.cpu.flags.left = 0;
                    expected.cpu.flags.right = 1;
                }
                _ => unreachable!(),
            }
            expected.cpu.eip = 0x1002;
            expected.cpu.instruction_count = 0;
            for execution in [machine.run_step(), machine.run_block(1)] {
                assert_eq!(execution.exit, Exit::Dispatch(0x1002));
                assert_eq!(execution.state, expected, "{code:02x?}, {register:?}");
                assert_eq!(execution.dispatches, [(0x1002, expected.clone())]);
                assert!(execution.machine_unchanged);
            }
        }
    }
}

#[test]
fn byte_unary_forms_select_low_and_high_aliases_without_changing_other_bytes() {
    let aliases = [
        (Gpr32::Eax, 0),
        (Gpr32::Ecx, 0),
        (Gpr32::Edx, 0),
        (Gpr32::Ebx, 0),
        (Gpr32::Eax, 8),
        (Gpr32::Ecx, 8),
        (Gpr32::Edx, 8),
        (Gpr32::Ebx, 8),
    ];
    for (opcode, extension, result) in [
        (0xfe, 0x00, 0x81), // INC
        (0xfe, 0x08, 0x7f), // DEC
        (0xf6, 0x10, 0x7f), // NOT
        (0xf6, 0x18, 0x80), // NEG
    ] {
        for (index, (parent, shift)) in aliases.into_iter().enumerate() {
            let code = [opcode, 0xc0 | extension | index as u8];
            let mut machine = Machine::new(&code);
            machine.cpu.flags.kind = if opcode == 0xfe { 0 } else { 0xff };
            machine.cpu.flags.status.cf = 1;
            machine.cpu.registers[parent] = 0x9234_8080;
            let mut expected = machine.state();
            expected.cpu.registers[parent] = (0x9234_8080 & !(0xff << shift)) | (result << shift);
            match extension {
                0x00 => {
                    expected.cpu.flags.status = StatusFlags {
                        cf: 1,
                        pf: 1,
                        af: 0,
                        zf: 0,
                        sf: 1,
                        of: 0,
                    }
                }
                0x08 => {
                    expected.cpu.flags.status = StatusFlags {
                        cf: 1,
                        pf: 0,
                        af: 1,
                        zf: 0,
                        sf: 0,
                        of: 1,
                    }
                }
                0x10 => {}
                0x18 => {
                    expected.cpu.flags.kind = 1;
                    expected.cpu.flags.left = 0;
                    expected.cpu.flags.right = 0x80;
                }
                _ => unreachable!(),
            }
            expected.cpu.eip = 0x1002;
            expected.cpu.instruction_count = 0;
            for execution in [machine.run_step(), machine.run_block(1)] {
                assert_eq!(execution.exit, Exit::Dispatch(0x1002));
                assert_eq!(
                    execution.state, expected,
                    "{code:02x?}, {parent:?}, shift {shift}"
                );
                assert_eq!(execution.dispatches, [(0x1002, expected.clone())]);
                assert!(execution.machine_unchanged);
            }
        }
    }
}

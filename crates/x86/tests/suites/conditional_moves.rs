use wasm86_x86::{Gpr32, StatusFlags};

use crate::support::{
    arithmetic,
    machine::{both, Exit, Step},
    step::TestModule,
};

#[path = "conditional_moves/decoding.rs"]
mod decoding;
#[path = "conditional_moves/memory.rs"]
mod memory;

#[test]
fn every_condition_selects_or_preserves_both_destination_widths() {
    // The literal outcome masks match the shared condition tests: bit n is
    // condition n (O through G). These three records exercise both outcomes
    // of every condition, including signed and unsigned comparisons.
    for (name, status, outcomes) in [
        (
            "condition inputs clear",
            StatusFlags {
                cf: 0,
                pf: 0,
                af: 1,
                zf: 0,
                sf: 0,
                of: 0,
            },
            0xaaaa_u16,
        ),
        (
            "carry and overflow",
            StatusFlags {
                cf: 1,
                pf: 1,
                af: 1,
                zf: 1,
                sf: 0,
                of: 1,
            },
            0x5655,
        ),
        (
            "negative without overflow",
            StatusFlags {
                cf: 0,
                pf: 0,
                af: 1,
                zf: 0,
                sf: 1,
                of: 0,
            },
            0x59aa,
        ),
    ] {
        for word in [false, true] {
            let mut image = arithmetic::image(&[]);
            image.cpu.flags.status = status;
            image.cpu.registers.eax = 0x4433_2211;
            image.cpu.registers.ecx = 0x8877_6655;
            let mut code = Vec::new();
            let mut steps = Vec::new();
            let mut cpu = image.cpu;
            for condition in 0..16 {
                // Resetting EAX keeps a false result observable even after
                // the preceding condition has copied the same source.
                if condition != 0 {
                    code.extend_from_slice(&[0xb8, 0x11, 0x22, 0x33, 0x44]);
                    cpu.registers.eax = 0x4433_2211;
                    cpu.eip = 0x1000 + code.len() as u32;
                    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
                    steps.push(Step {
                        cpu,
                        ram: &[],
                        exit: Exit::Dispatch(cpu.eip),
                    });
                }
                if word {
                    code.push(0x66);
                }
                code.extend_from_slice(&[0x0f, 0x40 + condition, 0xc1]);
                if outcomes & (1 << condition) != 0 {
                    cpu.registers.eax = if word { 0x4433_6655 } else { 0x8877_6655 };
                }
                cpu.eip = 0x1000 + code.len() as u32;
                cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
                steps.push(Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                });
            }
            image.data(0x3000, &code);
            both(
                TestModule::interpreter(),
                &format!("{name}, word destination {word}"),
                &code,
                steps.len() as u32,
                &image,
                &steps,
            );
        }
    }
}

#[test]
fn upper_register_codes_name_word_or_dword_registers() {
    for (code, destination, value) in [
        (&[0x66, 0x0f, 0x44, 0xe5][..], Gpr32::Esp, 0x5555_6666),
        (&[0x0f, 0x44, 0xee][..], Gpr32::Ebp, 0x7777_7777),
        (&[0x66, 0x0f, 0x44, 0xf7][..], Gpr32::Esi, 0x7777_8888),
        (&[0x0f, 0x44, 0xfc][..], Gpr32::Edi, 0x5555_5555),
    ] {
        let image = arithmetic::image(code);
        let mut cpu = image.cpu;
        cpu.registers[destination] = value;
        cpu.eip += code.len() as u32;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            "SP/BP/SI/DI register codes",
            code,
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

#[test]
fn incoming_lazy_records_are_read_without_materializing_flags() {
    for (name, kind, left, right, opcode, eax) in [
        ("byte ADD overflow", 2, 0x80, 0x80, 0x40, 0x8877_6655),
        (
            "word SUB signed comparison",
            5,
            0x7ffe,
            0xfffe,
            0x4c,
            0x4433_2211,
        ),
        ("dword logical zero", 11, 0, 0x1234_5678, 0x44, 0x8877_6655),
    ] {
        let code = [0x0f, opcode, 0xc1];
        let mut image = arithmetic::image(&code);
        image.cpu.flags.kind = kind;
        image.cpu.flags.left = left;
        image.cpu.flags.right = right;
        image.cpu.registers.eax = 0x4433_2211;
        image.cpu.registers.ecx = 0x8877_6655;
        let mut cpu = image.cpu;
        cpu.registers.eax = eax;
        cpu.eip = 0x1003;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            name,
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

#[test]
fn self_sources_and_mixed_aliases_keep_the_current_register_value() {
    let code = [
        0x66, 0x0f, 0x44, 0xc0, // CMOVE AX, AX
        0xb4, 0x80, // MOV AH, 0x80
        0x66, 0x0f, 0x44, 0xc2, // CMOVE AX, DX
        0x0f, 0x45, 0xc1, // CMOVNE EAX, ECX, false
        0x0f, 0xb6, 0xcc, // MOVZX ECX, AH
        0x0f, 0x44, 0xc1, // CMOVE EAX, ECX
    ];
    let mut image = arithmetic::image(&code);
    image.cpu.registers.eax = 0x4433_2211;
    image.cpu.registers.edx = 0xdead_c0de;
    let mut cpu = image.cpu;
    let mut steps = Vec::new();
    for (count, (next, eax, ecx)) in [
        (0x1004, 0x4433_2211, 0x2222_2222),
        (0x1006, 0x4433_8011, 0x2222_2222),
        (0x100a, 0x4433_c0de, 0x2222_2222),
        (0x100d, 0x4433_c0de, 0x2222_2222),
        (0x1010, 0x4433_c0de, 0x0000_00c0),
        (0x1013, 0x0000_00c0, 0x0000_00c0),
    ]
    .into_iter()
    .enumerate()
    {
        cpu.registers.eax = eax;
        cpu.registers.ecx = ecx;
        cpu.eip = next;
        cpu.instruction_count = count as u32;
        steps.push(Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(next),
        });
    }
    both(
        TestModule::interpreter(),
        "conditional mixed aliases",
        &code,
        6,
        &image,
        &steps,
    );
}

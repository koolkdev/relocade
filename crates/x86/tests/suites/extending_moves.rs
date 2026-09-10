use wasm86_x86::Gpr32;

use crate::support::machine::{both, byte_register_image, Exit, Step};
use crate::support::step::TestModule;

#[path = "extending_moves/decoding.rs"]
mod decoding;
#[path = "extending_moves/memory.rs"]
mod memory;

#[test]
fn every_legacy_byte_source_uses_its_low_or_high_byte() {
    let step = TestModule::interpreter();
    // Destination codes 4–7 name full registers while the same source codes
    // name AH–BH. Alternating widths also checks preservation of upper halves.
    for (code, name, destination, zero, signed) in [
        (0, "AL", Gpr32::Eax, 0x11, 0x0000_0011),
        (1, "CL", Gpr32::Ecx, 0x55, 0x0000_0055),
        (2, "DL", Gpr32::Edx, 0x99, 0xffff_ff99),
        (3, "BL", Gpr32::Ebx, 0xdd, 0xffff_ffdd),
        (4, "AH", Gpr32::Esp, 0x22, 0x0000_0022),
        (5, "CH", Gpr32::Ebp, 0x66, 0x0000_0066),
        (6, "DH", Gpr32::Esi, 0xaa, 0xffff_ffaa),
        (7, "BH", Gpr32::Edi, 0xee, 0xffff_ffee),
    ] {
        for (opcode, value, word) in [(0xb6, zero, code % 2 == 0), (0xbe, signed, code % 2 != 0)] {
            let mut bytes = if word { vec![0x66] } else { vec![] };
            bytes.extend_from_slice(&[0x0f, opcode, 0xc0 | (code << 3) | code]);
            let image = byte_register_image(&bytes);
            let mut expected = image.cpu;
            expected.registers[destination] = if word {
                (expected.registers[destination] & 0xffff_0000) | (value & 0xffff)
            } else {
                value
            };
            expected.eip += bytes.len() as u32;
            expected.instruction_count = 0;
            both(
                step,
                &format!("{name}, opcode {opcode:02x}, word destination {word}"),
                &bytes,
                1,
                &image,
                &[Step {
                    cpu: expected,
                    ram: &[],
                    exit: Exit::Dispatch(expected.eip),
                }],
            );
        }
    }
}

#[test]
fn sign_boundaries_preserve_flags_and_retire_once() {
    let step = TestModule::interpreter();
    for (code, source, eax) in [
        (&[0x0f, 0xbe, 0xc3][..], 0x00, 0x0000_0000),
        (&[0x66, 0x0f, 0xbe, 0xc3][..], 0x7f, 0x4433_007f),
        (&[0x0f, 0xbe, 0xc3][..], 0x80, 0xffff_ff80),
        (&[0x66, 0x0f, 0xbe, 0xc3][..], 0xff, 0x4433_ffff),
        (&[0x0f, 0xbf, 0xc3][..], 0x0000, 0x0000_0000),
        (&[0x0f, 0xbf, 0xc3][..], 0x7fff, 0x0000_7fff),
        (&[0x0f, 0xbf, 0xc3][..], 0x8000, 0xffff_8000),
        (&[0x0f, 0xbf, 0xc3][..], 0xffff, 0xffff_ffff),
    ] {
        let mut image = byte_register_image(code);
        image.cpu.registers.ebx = 0xa5a5_0000 | source;
        image.cpu.instruction_count = 41;
        let mut expected = image.cpu;
        expected.registers.eax = eax;
        expected.eip += code.len() as u32;
        expected.instruction_count = 42;
        both(
            step,
            &format!("sign boundary {source:04x} via {code:02x?}"),
            code,
            1,
            &image,
            &[Step {
                cpu: expected,
                ram: &[],
                exit: Exit::Dispatch(expected.eip),
            }],
        );
    }
}

#[test]
fn word_sources_zero_extend_or_keep_the_destination_upper_half() {
    let step = TestModule::interpreter();
    for (code, eax) in [
        (&[0x0f, 0xb7, 0xc2][..], 0x0000_8000),
        (&[0x66, 0x0f, 0xb7, 0xc2][..], 0x4433_8000),
        (&[0x66, 0x0f, 0xbf, 0xc2][..], 0x4433_8000),
    ] {
        let mut image = byte_register_image(code);
        image.cpu.registers.edx = 0x7fff_8000;
        let mut expected = image.cpu;
        expected.registers.eax = eax;
        expected.eip += code.len() as u32;
        expected.instruction_count = 0;
        both(
            step,
            "word source uses its low half",
            code,
            1,
            &image,
            &[Step {
                cpu: expected,
                ram: &[],
                exit: Exit::Dispatch(expected.eip),
            }],
        );
    }
}

#[test]
fn mixed_aliases_read_the_source_before_replacing_the_destination() {
    let step = TestModule::interpreter();
    let code = [
        0xb4, 0x80, // MOV AH, 0x80
        0x66, 0x66, 0x0f, 0xbe, 0xc4, // MOVSX AX, AH, repeated override
        0x0f, 0xb7, 0xd0, // MOVZX EDX, AX
        0x0f, 0xbf, 0xc0, // MOVSX EAX, AX
        0x0f, 0xb6, 0xcc, // MOVZX ECX, AH
        0xb4, 0x7f, // MOV AH, 0x7f
        0x66, 0x0f, 0xb6, 0xc0, // MOVZX AX, AL
        0x0f, 0xbf, 0xf0, // MOVSX ESI, AX
        0x0f, 0xb6, 0xc0, // MOVZX EAX, AL
    ];
    let image = byte_register_image(&code);
    let mut expected = image.cpu;
    let mut steps = Vec::new();
    for (index, (next, destination, value)) in [
        (0x1002, Gpr32::Eax, 0x4433_8011),
        (0x1007, Gpr32::Eax, 0x4433_ff80),
        (0x100a, Gpr32::Edx, 0x0000_ff80),
        (0x100d, Gpr32::Eax, 0xffff_ff80),
        (0x1010, Gpr32::Ecx, 0x0000_00ff),
        (0x1012, Gpr32::Eax, 0xffff_7f80),
        (0x1016, Gpr32::Eax, 0xffff_0080),
        (0x1019, Gpr32::Esi, 0x0000_0080),
        (0x101c, Gpr32::Eax, 0x0000_0080),
    ]
    .into_iter()
    .enumerate()
    {
        expected.registers[destination] = value;
        expected.eip = next;
        expected.instruction_count = index as u32;
        steps.push(Step {
            cpu: expected,
            ram: &[],
            exit: Exit::Dispatch(next),
        });
    }
    both(step, "mixed register aliases", &code, 9, &image, &steps);
}

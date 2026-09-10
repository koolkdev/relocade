use wasm86_x86::Gpr32;

use crate::support::{
    machine::{both, byte_register_image, Exit, Image, Step},
    step::TestModule,
};

#[path = "exchanges/decoding.rs"]
mod decoding;
#[path = "exchanges/memory.rs"]
mod memory;

fn image(code: &[u8]) -> Image {
    let mut image = byte_register_image(code);
    image.cpu.flags.kind = 9;
    image.cpu.flags.left = 0x7fff_fffe;
    image.cpu.flags.right = 0xffff_fffe;
    image
}

fn register_exchange(name: &str, code: &[u8], changes: &[(Gpr32, u32)]) {
    let image = image(code);
    let mut cpu = image.cpu;
    for &(register, value) in changes {
        cpu.registers[register] = value;
    }
    cpu.eip += code.len() as u32;
    cpu.instruction_count = 0;
    both(
        TestModule::interpreter(),
        name,
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

#[test]
fn byte_register_forms_exchange_old_values_including_shared_parents() {
    struct Case {
        name: &'static str,
        code: &'static [u8],
        changes: &'static [(Gpr32, u32)],
    }
    for case in [
        Case {
            name: "AL and AH share EAX",
            code: &[0x86, 0xc4],
            changes: &[(Gpr32::Eax, 0x4433_1122)],
        },
        Case {
            name: "CL and DL",
            code: &[0x86, 0xca],
            changes: &[(Gpr32::Ecx, 0x8877_6699), (Gpr32::Edx, 0xccbb_aa55)],
        },
        Case {
            name: "DL and BH",
            code: &[0x86, 0xd7],
            changes: &[(Gpr32::Edx, 0xccbb_aaee), (Gpr32::Ebx, 0x10ff_99dd)],
        },
        Case {
            name: "BL and CH",
            code: &[0x86, 0xdd],
            changes: &[(Gpr32::Ebx, 0x10ff_ee66), (Gpr32::Ecx, 0x8877_dd55)],
        },
        Case {
            name: "AH and DH",
            code: &[0x86, 0xe6],
            changes: &[(Gpr32::Eax, 0x4433_aa11), (Gpr32::Edx, 0xccbb_2299)],
        },
        Case {
            name: "CH and AH",
            code: &[0x86, 0xec],
            changes: &[(Gpr32::Ecx, 0x8877_2255), (Gpr32::Eax, 0x4433_6611)],
        },
        Case {
            name: "operand-size override keeps the byte form",
            code: &[0x66, 0x86, 0xf0],
            changes: &[(Gpr32::Edx, 0xccbb_1199), (Gpr32::Eax, 0x4433_22aa)],
        },
        Case {
            name: "BH exchanges with itself",
            code: &[0x86, 0xff],
            changes: &[],
        },
    ] {
        register_exchange(case.name, case.code, case.changes);
    }
}

#[test]
fn accumulator_forms_cover_every_register_and_both_nop_aliases() {
    for (opcode, register, dword, word_eax, word_register) in [
        (0x90, Gpr32::Eax, 0x4433_2211, 0x4433_2211, 0x4433_2211),
        (0x91, Gpr32::Ecx, 0x8877_6655, 0x4433_6655, 0x8877_2211),
        (0x92, Gpr32::Edx, 0xccbb_aa99, 0x4433_aa99, 0xccbb_2211),
        (0x93, Gpr32::Ebx, 0x10ff_eedd, 0x4433_eedd, 0x10ff_2211),
        (0x94, Gpr32::Esp, 0x5555_5555, 0x4433_5555, 0x5555_2211),
        (0x95, Gpr32::Ebp, 0x6666_6666, 0x4433_6666, 0x6666_2211),
        (0x96, Gpr32::Esi, 0x7777_7777, 0x4433_7777, 0x7777_2211),
        (0x97, Gpr32::Edi, 0x8888_8888, 0x4433_8888, 0x8888_2211),
    ] {
        register_exchange(
            &format!("dword accumulator opcode {opcode:02x}"),
            &[opcode],
            &[(register, 0x4433_2211), (Gpr32::Eax, dword)],
        );
        register_exchange(
            &format!("word accumulator opcode {opcode:02x}"),
            &[0x66, opcode],
            &[(register, word_register), (Gpr32::Eax, word_eax)],
        );
    }
}

#[test]
fn general_word_and_dword_register_forms_preserve_unwritten_bits() {
    register_exchange(
        "SI and BP keep their upper halves",
        &[0x66, 0x87, 0xf5],
        &[(Gpr32::Esi, 0x7777_6666), (Gpr32::Ebp, 0x6666_7777)],
    );
    register_exchange(
        "EDI and EAX exchange all four bytes",
        &[0x87, 0xf8],
        &[(Gpr32::Edi, 0x4433_2211), (Gpr32::Eax, 0x8888_8888)],
    );
    register_exchange("ESP exchanges with itself", &[0x87, 0xe4], &[]);
}

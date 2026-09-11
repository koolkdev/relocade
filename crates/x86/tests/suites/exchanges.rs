#[path = "exchanges/sequences.rs"]
mod sequences;

use crate::support::cases::{test_cases, InstructionCase as Case, RegisterExpectation::Exact};

use wasm86_x86::{Gpr32, StatusFlags, StoredFlags};

use crate::support::machine::{byte_register_image, Image};

#[path = "exchanges/decoding.rs"]
mod decoding;
#[path = "exchanges/memory.rs"]
mod memory;

const LAZY_FLAGS: StoredFlags = StoredFlags {
    kind: 9,
    reserved: [0xa5; 3],
    left: 0x7fff_fffe,
    right: 0xffff_fffe,
    status: StatusFlags {
        cf: 0xa5,
        pf: 0xa5,
        af: 0xa5,
        zf: 0xa5,
        sf: 0xa5,
        of: 0xa5,
    },
    non_status: [0xa5; 6],
};

fn image(code: &[u8]) -> Image {
    let mut image = byte_register_image(code);
    image.cpu.flags = LAZY_FLAGS;
    image
}

#[rustfmt::skip]
const INPUTS: &[(Gpr32, u32)] = &[
    (Gpr32::Eax, 0x4433_2211), (Gpr32::Ecx, 0x8877_6655),
    (Gpr32::Edx, 0xccbb_aa99), (Gpr32::Ebx, 0x10ff_eedd),
    (Gpr32::Esp, 0x5555_5555), (Gpr32::Ebp, 0x6666_6666),
    (Gpr32::Esi, 0x7777_7777), (Gpr32::Edi, 0x8888_8888),
];

#[rustfmt::skip]
fn byte_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("XCHG AL,AH: shared parent", &[0x86, 0xc4]).stored_flags(LAZY_FLAGS)
            .register(Gpr32::Eax, 0x4433_2211, 0x4433_1122),
        Case::preserving_flags("XCHG CL,DL: low bytes", &[0x86, 0xca]).stored_flags(LAZY_FLAGS)
            .register(Gpr32::Ecx, 0x8877_6655, 0x8877_6699).register(Gpr32::Edx, 0xccbb_aa99, 0xccbb_aa55),
        Case::preserving_flags("XCHG DL,BH: low and high bytes", &[0x86, 0xd7]).stored_flags(LAZY_FLAGS)
            .register(Gpr32::Edx, 0xccbb_aa99, 0xccbb_aaee).register(Gpr32::Ebx, 0x10ff_eedd, 0x10ff_99dd),
        Case::preserving_flags("XCHG BL,CH: low and high bytes", &[0x86, 0xdd]).stored_flags(LAZY_FLAGS)
            .register(Gpr32::Ebx, 0x10ff_eedd, 0x10ff_ee66).register(Gpr32::Ecx, 0x8877_6655, 0x8877_dd55),
        Case::preserving_flags("XCHG AH,DH: high bytes", &[0x86, 0xe6]).stored_flags(LAZY_FLAGS)
            .register(Gpr32::Eax, 0x4433_2211, 0x4433_aa11).register(Gpr32::Edx, 0xccbb_aa99, 0xccbb_2299),
        Case::preserving_flags("XCHG CH,AH: high bytes", &[0x86, 0xec]).stored_flags(LAZY_FLAGS)
            .register(Gpr32::Ecx, 0x8877_6655, 0x8877_2255).register(Gpr32::Eax, 0x4433_2211, 0x4433_6611),
        Case::preserving_flags("XCHG DH,AL: override retains byte width", &[0x66, 0x86, 0xf0]).stored_flags(LAZY_FLAGS)
            .register(Gpr32::Edx, 0xccbb_aa99, 0xccbb_1199).register(Gpr32::Eax, 0x4433_2211, 0x4433_22aa),
        Case::preserving_flags("XCHG BH,BH: self exchange", &[0x86, 0xff]).stored_flags(LAZY_FLAGS)
            .initial_register(Gpr32::Ebx, 0x10ff_eedd),
    ]
}
test_cases!(
    byte_register_forms_exchange_old_values_including_shared_parents,
    byte_cases()
);

#[rustfmt::skip]
fn accumulator_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    struct Accumulator { opcode: u8, register: Gpr32, dword_eax: u32, word_eax: u32, word_register: u32 }
    for row in [
        Accumulator { opcode: 0x90, register: Gpr32::Eax, dword_eax: 0x4433_2211, word_eax: 0x4433_2211, word_register: 0x4433_2211 },
        Accumulator { opcode: 0x91, register: Gpr32::Ecx, dword_eax: 0x8877_6655, word_eax: 0x4433_6655, word_register: 0x8877_2211 },
        Accumulator { opcode: 0x92, register: Gpr32::Edx, dword_eax: 0xccbb_aa99, word_eax: 0x4433_aa99, word_register: 0xccbb_2211 },
        Accumulator { opcode: 0x93, register: Gpr32::Ebx, dword_eax: 0x10ff_eedd, word_eax: 0x4433_eedd, word_register: 0x10ff_2211 },
        Accumulator { opcode: 0x94, register: Gpr32::Esp, dword_eax: 0x5555_5555, word_eax: 0x4433_5555, word_register: 0x5555_2211 },
        Accumulator { opcode: 0x95, register: Gpr32::Ebp, dword_eax: 0x6666_6666, word_eax: 0x4433_6666, word_register: 0x6666_2211 },
        Accumulator { opcode: 0x96, register: Gpr32::Esi, dword_eax: 0x7777_7777, word_eax: 0x4433_7777, word_register: 0x7777_2211 },
        Accumulator { opcode: 0x97, register: Gpr32::Edi, dword_eax: 0x8888_8888, word_eax: 0x4433_8888, word_register: 0x8888_2211 },
    ] {
        let mut dword_case = Case::preserving_flags(format!("dword accumulator opcode {:02x}", row.opcode), &[row.opcode])
            .stored_flags(LAZY_FLAGS).initial_registers(INPUTS).expect_register(Gpr32::Eax, Exact(row.dword_eax));
        let mut word_case = Case::preserving_flags(format!("word accumulator opcode {:02x}", row.opcode), &[0x66, row.opcode])
            .stored_flags(LAZY_FLAGS).initial_registers(INPUTS).expect_register(Gpr32::Eax, Exact(row.word_eax));
        if row.register != Gpr32::Eax {
            dword_case = dword_case.expect_register(row.register, Exact(0x4433_2211));
            word_case = word_case.expect_register(row.register, Exact(row.word_register));
        }
        cases.extend([dword_case, word_case]);
    }
    cases
}
test_cases!(
    accumulator_forms_cover_every_register_and_both_nop_aliases,
    accumulator_cases()
);

#[rustfmt::skip]
fn general_register_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("XCHG SI,BP: upper halves preserved", &[0x66, 0x87, 0xf5]).stored_flags(LAZY_FLAGS)
            .register(Gpr32::Esi, 0x7777_7777, 0x7777_6666).register(Gpr32::Ebp, 0x6666_6666, 0x6666_7777),
        Case::preserving_flags("XCHG EDI,EAX: all four bytes", &[0x87, 0xf8]).stored_flags(LAZY_FLAGS)
            .register(Gpr32::Edi, 0x8888_8888, 0x4433_2211).register(Gpr32::Eax, 0x4433_2211, 0x8888_8888),
        Case::preserving_flags("XCHG ESP,ESP: self exchange", &[0x87, 0xe4]).stored_flags(LAZY_FLAGS)
            .initial_register(Gpr32::Esp, 0x5555_5555),
    ]
}
test_cases!(
    general_word_and_dword_register_forms_preserve_unwritten_bits,
    general_register_cases()
);

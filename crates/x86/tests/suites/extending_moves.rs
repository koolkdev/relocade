#[path = "extending_moves/sequences.rs"]
mod sequences;

use crate::support::cases::{test_cases, InstructionCase as Case};

use wasm86_x86::Gpr32;

#[path = "extending_moves/decoding.rs"]
mod decoding;
#[path = "extending_moves/memory.rs"]
mod memory;

#[rustfmt::skip]
fn legacy_source_cases() -> Vec<Case> {
    struct Source { code: u8, name: &'static str, source: Gpr32, input: u32, destination: Gpr32, destination_input: u32, zero: u32, signed: u32 }
    let sources = [
        Source { code: 0, name: "AL", source: Gpr32::Eax, input: 0x4433_2211, destination: Gpr32::Eax, destination_input: 0x4433_2211, zero: 0x4433_0011, signed: 0x0000_0011 },
        Source { code: 1, name: "CL", source: Gpr32::Ecx, input: 0x8877_6655, destination: Gpr32::Ecx, destination_input: 0x8877_6655, zero: 0x0000_0055, signed: 0x8877_0055 },
        Source { code: 2, name: "DL", source: Gpr32::Edx, input: 0xccbb_aa99, destination: Gpr32::Edx, destination_input: 0xccbb_aa99, zero: 0xccbb_0099, signed: 0xffff_ff99 },
        Source { code: 3, name: "BL", source: Gpr32::Ebx, input: 0x10ff_eedd, destination: Gpr32::Ebx, destination_input: 0x10ff_eedd, zero: 0x0000_00dd, signed: 0x10ff_ffdd },
        Source { code: 4, name: "AH", source: Gpr32::Eax, input: 0x4433_2211, destination: Gpr32::Esp, destination_input: 0x5555_5555, zero: 0x5555_0022, signed: 0x0000_0022 },
        Source { code: 5, name: "CH", source: Gpr32::Ecx, input: 0x8877_6655, destination: Gpr32::Ebp, destination_input: 0x6666_6666, zero: 0x0000_0066, signed: 0x6666_0066 },
        Source { code: 6, name: "DH", source: Gpr32::Edx, input: 0xccbb_aa99, destination: Gpr32::Esi, destination_input: 0x7777_7777, zero: 0x7777_00aa, signed: 0xffff_ffaa },
        Source { code: 7, name: "BH", source: Gpr32::Ebx, input: 0x10ff_eedd, destination: Gpr32::Edi, destination_input: 0x8888_8888, zero: 0x0000_00ee, signed: 0x8888_ffee },
    ];
    let mut cases = Vec::new();
    for source in sources {
        for (opcode, output, word) in [(0xb6, source.zero, source.code % 2 == 0), (0xbe, source.signed, source.code % 2 != 0)] {
            let mut code = if word { vec![0x66] } else { vec![] };
            code.extend_from_slice(&[0x0f, opcode, 0xc0 | (source.code << 3) | source.code]);
            let mut case = Case::preserving_flags(format!("{} via {opcode:02x}, word destination {word}", source.name), &code)
                .register(source.destination, source.destination_input, output);
            if source.source != source.destination {
                case = case.initial_register(source.source, source.input);
            }
            cases.push(case);
        }
    }
    cases
}
test_cases!(
    every_legacy_byte_source_uses_its_low_or_high_byte,
    legacy_source_cases()
);

#[rustfmt::skip]
fn sign_boundary_cases() -> Vec<Case> {
    [
        (&[0x0f, 0xbe, 0xc3][..], 0xa5a5_0000, 0x0000_0000),
        (&[0x66, 0x0f, 0xbe, 0xc3][..], 0xa5a5_007f, 0x4433_007f),
        (&[0x0f, 0xbe, 0xc3][..], 0xa5a5_0080, 0xffff_ff80),
        (&[0x66, 0x0f, 0xbe, 0xc3][..], 0xa5a5_00ff, 0x4433_ffff),
        (&[0x0f, 0xbf, 0xc3][..], 0xa5a5_0000, 0x0000_0000),
        (&[0x0f, 0xbf, 0xc3][..], 0xa5a5_7fff, 0x0000_7fff),
        (&[0x0f, 0xbf, 0xc3][..], 0xa5a5_8000, 0xffff_8000),
        (&[0x0f, 0xbf, 0xc3][..], 0xa5a5_ffff, 0xffff_ffff),
    ].into_iter().map(|(code, ebx, eax)| {
        Case::preserving_flags(format!("MOVSX sign boundary {ebx:08x} via {code:02x?}"), code).instruction_count(41)
            .initial_register(Gpr32::Ebx, ebx).register(Gpr32::Eax, 0x4433_2211, eax)
    }).collect()
}
test_cases!(
    sign_boundaries_preserve_flags_and_retire_once,
    sign_boundary_cases()
);

#[rustfmt::skip]
fn word_source_cases() -> Vec<Case> {
    [
        (&[0x0f, 0xb7, 0xc2][..], 0x0000_8000),
        (&[0x66, 0x0f, 0xb7, 0xc2][..], 0x4433_8000),
        (&[0x66, 0x0f, 0xbf, 0xc2][..], 0x4433_8000),
    ].into_iter().map(|(code, eax)| {
        Case::preserving_flags(format!("word source uses its low half: {code:02x?}"), code)
            .initial_register(Gpr32::Edx, 0x7fff_8000).register(Gpr32::Eax, 0x4433_2211, eax)
    }).collect()
}
test_cases!(
    word_sources_zero_extend_or_keep_the_destination_upper_half,
    word_source_cases()
);

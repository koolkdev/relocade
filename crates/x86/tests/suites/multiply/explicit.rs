use wasm86_x86::Gpr32::{self, Eax, Ebx};

use crate::support::cases::{
    test_cases,
    FlagExpectation::{self, Clear, Set},
    InstructionCase as Case,
};

use super::product_flags;

struct Edge {
    name: &'static str,
    word: bool,
    source: u32,
    multiplier: u32,
    immediate8: Option<u8>,
    product: u32,
    overflow: FlagExpectation,
}

// Source and product include the untouched upper EAX half for word destinations.
#[rustfmt::skip]
fn edge_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for edge in [
        Edge { name: "word zero", word: true, source: 0x4433_0000, multiplier: 0xffff,
            immediate8: Some(0xff), product: 0x4433_0000, overflow: Clear },
        Edge { name: "word negative one", word: true, source: 0x4433_0001, multiplier: 0xffff,
            immediate8: Some(0xff), product: 0x4433_ffff, overflow: Clear },
        Edge { name: "word minimum fits", word: true, source: 0x4433_8000, multiplier: 1,
            immediate8: Some(1), product: 0x4433_8000, overflow: Clear },
        Edge { name: "word minimum negated overflows", word: true, source: 0x4433_8000, multiplier: 0xffff,
            immediate8: Some(0xff), product: 0x4433_8000, overflow: Set },
        Edge { name: "word doubled maximum overflows", word: true, source: 0x4433_7fff, multiplier: 2,
            immediate8: Some(2), product: 0x4433_fffe, overflow: Set },
        Edge { name: "word two negative factors", word: true, source: 0x4433_fffd, multiplier: 0xfffe,
            immediate8: Some(0xfe), product: 0x4433_0006, overflow: Clear },
        Edge { name: "word positive 128 overflows", word: true, source: 0x4433_0100, multiplier: 0x80,
            immediate8: None, product: 0x4433_8000, overflow: Set },
        Edge { name: "word positive 127 fits", word: true, source: 0x4433_0100, multiplier: 0x7f,
            immediate8: Some(0x7f), product: 0x4433_7f00, overflow: Clear },
        Edge { name: "word negative 128 fits", word: true, source: 0x4433_0100, multiplier: 0xff80,
            immediate8: Some(0x80), product: 0x4433_8000, overflow: Clear },
        Edge { name: "word negative 128 overflows", word: true, source: 0x4433_0101, multiplier: 0xff80,
            immediate8: Some(0x80), product: 0x4433_7f80, overflow: Set },
        Edge { name: "word zero low product still overflows", word: true, source: 0x4433_8000, multiplier: 0x8000,
            immediate8: None, product: 0x4433_0000, overflow: Set },
        Edge { name: "dword zero", word: false, source: 0, multiplier: 0xffff_ffff,
            immediate8: Some(0xff), product: 0, overflow: Clear },
        Edge { name: "dword negative one", word: false, source: 1, multiplier: 0xffff_ffff,
            immediate8: Some(0xff), product: 0xffff_ffff, overflow: Clear },
        Edge { name: "dword minimum fits", word: false, source: 0x8000_0000, multiplier: 1,
            immediate8: Some(1), product: 0x8000_0000, overflow: Clear },
        Edge { name: "dword minimum negated overflows", word: false, source: 0x8000_0000, multiplier: 0xffff_ffff,
            immediate8: Some(0xff), product: 0x8000_0000, overflow: Set },
        Edge { name: "dword doubled maximum overflows", word: false, source: 0x7fff_ffff, multiplier: 2,
            immediate8: Some(2), product: 0xffff_fffe, overflow: Set },
        Edge { name: "dword two negative factors", word: false, source: 0xffff_fffd, multiplier: 0xffff_fffe,
            immediate8: Some(0xfe), product: 6, overflow: Clear },
        Edge { name: "dword positive 128 overflows", word: false, source: 0x0100_0000, multiplier: 0x80,
            immediate8: None, product: 0x8000_0000, overflow: Set },
        Edge { name: "dword positive 127 fits", word: false, source: 0x0100_0000, multiplier: 0x7f,
            immediate8: Some(0x7f), product: 0x7f00_0000, overflow: Clear },
        Edge { name: "dword negative 128 fits", word: false, source: 0x0100_0000, multiplier: 0xffff_ff80,
            immediate8: Some(0x80), product: 0x8000_0000, overflow: Clear },
        Edge { name: "dword negative 128 overflows", word: false, source: 0x0100_0001, multiplier: 0xffff_ff80,
            immediate8: Some(0x80), product: 0x7fff_ff80, overflow: Set },
        Edge { name: "dword zero low product still overflows", word: false, source: 0x8000_0000, multiplier: 0x8000_0000,
            immediate8: None, product: 0, overflow: Set },
    ] {
        let prefix = if edge.word { vec![0x66] } else { vec![] };
        let mut two = prefix.clone();
        two.extend_from_slice(&[0x0f, 0xaf, 0xc3]);
        cases.push(Case::replacing_flags(format!("IMUL two operands: {}", edge.name), &two, product_flags(edge.overflow))
            .register(Eax, edge.source, edge.product).initial_register(Ebx, edge.multiplier));
        let mut wide = prefix.clone();
        wide.extend_from_slice(&[0x69, 0xc3]);
        wide.extend_from_slice(&edge.multiplier.to_le_bytes()[..if edge.word { 2 } else { 4 }]);
        cases.push(Case::replacing_flags(format!("IMUL wide immediate: {}", edge.name), &wide, product_flags(edge.overflow))
            .register(Eax, 0x4433_dead, edge.product).initial_register(Ebx, edge.source));
        if let Some(immediate) = edge.immediate8 {
            let mut short = prefix;
            short.extend_from_slice(&[0x6b, 0xc3, immediate]);
            cases.push(Case::replacing_flags(format!("IMUL signed byte immediate: {}", edge.name), &short, product_flags(edge.overflow))
                .register(Eax, 0x4433_dead, edge.product).initial_register(Ebx, edge.source));
        }
    }
    cases
}

fn all_register_pairs() -> Vec<Case> {
    let mut cases = Vec::new();
    for (word, positive, negative, product, square) in [
        (true, 0x4433_0002, 0x4433_fffd, 0x4433_fffa, 0x4433_0009),
        (false, 2, 0xffff_fffd, 0xffff_fffa, 9),
    ] {
        for (destination_code, destination) in Gpr32::ALL.into_iter().enumerate() {
            for (source_code, source) in Gpr32::ALL.into_iter().enumerate() {
                let mut code = if word { vec![0x66] } else { vec![] };
                code.extend_from_slice(&[
                    0x0f,
                    0xaf,
                    0xc0 | ((destination_code as u8) << 3) | source_code as u8,
                ]);
                let aliased = source == destination;
                let mut case = Case::replacing_flags(
                    format!(
                        "IMUL {} {destination:?},{source:?} reads old operands",
                        if word { "word" } else { "dword" }
                    ),
                    &code,
                    product_flags(Clear),
                )
                .register(
                    destination,
                    if aliased { negative } else { positive },
                    if aliased { square } else { product },
                );
                if !aliased {
                    case = case.initial_register(source, negative);
                }
                cases.push(case);
            }
        }
    }
    cases
}

fn immediate_register_selectors() -> Vec<Case> {
    let mut cases = Vec::new();
    for (word, source_input, product) in [(true, 0x4433_fffd, 0x4433_0006), (false, 0xffff_fffd, 6)]
    {
        for (destination_code, destination) in Gpr32::ALL.into_iter().enumerate() {
            for source_code in [destination_code, (destination_code + 1) % 8] {
                let source = Gpr32::ALL[source_code];
                for (opcode, immediate) in [
                    (
                        0x69,
                        if word {
                            &[0xfe, 0xff][..]
                        } else {
                            &[0xfe, 0xff, 0xff, 0xff][..]
                        },
                    ),
                    (0x6b, &[0xfe][..]),
                ] {
                    let mut code = if word { vec![0x66] } else { vec![] };
                    code.extend_from_slice(&[
                        opcode,
                        0xc0 | ((destination_code as u8) << 3) | source_code as u8,
                    ]);
                    code.extend_from_slice(immediate);
                    let mut case = Case::replacing_flags(
                        format!(
                            "IMUL {} opcode {opcode:02x} {destination:?},{source:?},-2",
                            if word { "word" } else { "dword" }
                        ),
                        &code,
                        product_flags(Clear),
                    )
                    .register(
                        destination,
                        if source == destination {
                            source_input
                        } else {
                            0x4433_dead
                        },
                        product,
                    );
                    if source != destination {
                        case = case.initial_register(source, source_input);
                    }
                    cases.push(case);
                }
            }
        }
    }
    cases
}

test_cases!(signed_fit_and_immediate_extension, edge_cases());
test_cases!(every_destination_and_source_pair, all_register_pairs());
test_cases!(
    immediate_forms_capture_source_before_destination,
    immediate_register_selectors()
);

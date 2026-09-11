use wasm86_x86::Gpr32::Eax;

use crate::support::cases::{
    test_cases,
    FlagExpectation::{self, Clear, Preserved, Set},
    Flags, InstructionCase as Case,
};

// Intel SDM 325462-089, Volume 2, INC/DEC instruction entries. These are
// hand-reviewed literal boundaries, including the complete parent EAX values.
// The INC/DEC rows retain the previous byte/word/dword and both-CF coverage.
// Array columns are AL, AX, and EAX.
struct Boundary {
    name: &'static str,
    input: [u32; 3],
    result: [u32; 3],
    parity: [FlagExpectation; 3],
    auxiliary: FlagExpectation,
    zero: FlagExpectation,
    sign: FlagExpectation,
    overflow: FlagExpectation,
}

const INCREMENTS: &[Boundary] = &[
    Boundary {
        name: "zero to one",
        input: [0x1234_5600, 0x1234_0000, 0],
        result: [0x1234_5601, 0x1234_0001, 1],
        parity: [Clear; 3],
        auxiliary: Clear,
        zero: Clear,
        sign: Clear,
        overflow: Clear,
    },
    Boundary {
        name: "nibble carry",
        input: [0x1234_560f, 0x1234_000f, 0x0f],
        result: [0x1234_5610, 0x1234_0010, 0x10],
        parity: [Clear; 3],
        auxiliary: Set,
        zero: Clear,
        sign: Clear,
        overflow: Clear,
    },
    Boundary {
        name: "signed overflow",
        input: [0x1234_567f, 0x1234_7fff, 0x7fff_ffff],
        result: [0x1234_5680, 0x1234_8000, 0x8000_0000],
        parity: [Clear, Set, Set],
        auxiliary: Set,
        zero: Clear,
        sign: Set,
        overflow: Set,
    },
    Boundary {
        name: "unsigned wrap",
        input: [0x1234_56ff, 0x1234_ffff, 0xffff_ffff],
        result: [0x1234_5600, 0x1234_0000, 0],
        parity: [Set; 3],
        auxiliary: Set,
        zero: Set,
        sign: Clear,
        overflow: Clear,
    },
];

const DECREMENTS: &[Boundary] = &[
    Boundary {
        name: "one to zero",
        input: [0x1234_5601, 0x1234_0001, 1],
        result: [0x1234_5600, 0x1234_0000, 0],
        parity: [Set; 3],
        auxiliary: Clear,
        zero: Set,
        sign: Clear,
        overflow: Clear,
    },
    Boundary {
        name: "nibble borrow",
        input: [0x1234_5610, 0x1234_0010, 0x10],
        result: [0x1234_560f, 0x1234_000f, 0x0f],
        parity: [Set; 3],
        auxiliary: Set,
        zero: Clear,
        sign: Clear,
        overflow: Clear,
    },
    Boundary {
        name: "signed overflow",
        input: [0x1234_5680, 0x1234_8000, 0x8000_0000],
        result: [0x1234_567f, 0x1234_7fff, 0x7fff_ffff],
        parity: [Clear, Set, Set],
        auxiliary: Set,
        zero: Clear,
        sign: Clear,
        overflow: Set,
    },
    Boundary {
        name: "unsigned wrap",
        input: [0x1234_5600, 0x1234_0000, 0],
        result: [0x1234_56ff, 0x1234_ffff, 0xffff_ffff],
        parity: [Set; 3],
        auxiliary: Set,
        zero: Clear,
        sign: Set,
        overflow: Clear,
    },
];

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (operation, encodings, boundaries) in [
        (
            "INC",
            [
                ("AL", &[0xfe, 0xc0][..]),
                ("AX", &[0x66, 0xff, 0xc0][..]),
                ("EAX", &[0xff, 0xc0][..]),
            ],
            INCREMENTS,
        ),
        (
            "DEC",
            [
                ("AL", &[0xfe, 0xc8][..]),
                ("AX", &[0x66, 0xff, 0xc8][..]),
                ("EAX", &[0xff, 0xc8][..]),
            ],
            DECREMENTS,
        ),
    ] {
        for (width, (register, code)) in encodings.into_iter().enumerate() {
            for boundary in boundaries {
                for carry in [false, true] {
                    cases.push(
                        Case::new(
                            format!("{operation} {register}: {}, CF {carry}", boundary.name),
                            code,
                            Flags {
                                cf: carry,
                                pf: true,
                                af: true,
                                zf: true,
                                sf: true,
                                of: true,
                            },
                            Flags {
                                cf: Preserved,
                                pf: boundary.parity[width],
                                af: boundary.auxiliary,
                                zf: boundary.zero,
                                sf: boundary.sign,
                                of: boundary.overflow,
                            },
                        )
                        .register(
                            Eax,
                            boundary.input[width],
                            boundary.result[width],
                        ),
                    );
                }
            }
        }
    }
    cases
}

test_cases!(inc_dec_boundaries, cases());

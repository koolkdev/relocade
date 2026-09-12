use crate::flags::Flag;
use wasm86_x86::{FlagBytes, StoredStatusSource};
#[path = "flag_control/carry.rs"]
mod carry;
#[path = "flag_control/decoding.rs"]
mod decoding;
#[path = "flag_control/direction.rs"]
mod direction;
#[path = "flag_control/sequences.rs"]
mod sequences;

use crate::support::cases::{FlagExpectation, Flags, InstructionCase as Case};
use wasm86_x86::{CpuState, StoredFlags};

const ENCODINGS: [(&str, u8); 5] = [
    ("CLC", 0xf8),
    ("STC", 0xf9),
    ("CMC", 0xf5),
    ("CLD", 0xfc),
    ("STD", 0xfd),
];

fn logical_flags(bits: u8) -> Flags<bool> {
    Flags {
        cf: bits & 1 != 0,
        pf: bits & 2 != 0,
        af: bits & 4 != 0,
        zf: bits & 8 != 0,
        sf: bits & 16 != 0,
        of: bits & 32 != 0,
    }
}

fn carry_result(value: bool) -> Flags<FlagExpectation> {
    Flags {
        cf: if value {
            FlagExpectation::Set
        } else {
            FlagExpectation::Clear
        },
        ..Flags::all(FlagExpectation::Preserved)
    }
}

fn operation_case(name: String, code: &[u8], opcode: u8) -> Case {
    match opcode {
        0xfc | 0xfd => {
            Case::preserving_flags(name, code).expect_direct_flag(Flag::DF, opcode == 0xfd)
        }
        0xf8 => Case::new(name, code, Flags::all(true), carry_result(false)),
        0xf9 | 0xf5 => Case::new(name, code, Flags::all(false), carry_result(true)),
        _ => unreachable!(),
    }
}

fn flag_records() -> Vec<(&'static str, StoredFlags, Flags<bool>)> {
    let status = Flags {
        cf: 0xfe,
        pf: 0xff,
        af: 0x80,
        zf: 0x7f,
        sf: 0x5a,
        of: 0x5b,
    };
    let base = StoredFlags {
        status_source: StoredStatusSource {
            kind: 0,
            left: 0x1234_5678,
            right: 0x8765_4321,
            ..(CpuState::filled(0xa5).flags).status_source
        },
        bytes: FlagBytes {
            cf: status.cf,
            pf: status.pf,
            af: status.af,
            zf: status.zf,
            sf: status.sf,
            of: status.of,
            ..(CpuState::filled(0xa5).flags).bytes
        },
    };
    let mut records = vec![
        (
            "noncanonical concrete clear carry",
            base,
            Flags {
                cf: false,
                pf: true,
                af: false,
                zf: true,
                sf: false,
                of: true,
            },
        ),
        (
            "noncanonical concrete set carry",
            StoredFlags {
                bytes: FlagBytes {
                    cf: 0x81,
                    pf: status.pf,
                    af: status.af,
                    zf: status.zf,
                    sf: status.sf,
                    of: status.of,
                    ..base.bytes
                },
                ..base
            },
            Flags {
                cf: true,
                pf: true,
                af: false,
                zf: true,
                sf: false,
                of: true,
            },
        ),
    ];
    for (name, kind, left, right, flags) in [
        (
            "pending byte ADD",
            2,
            0xff,
            1,
            Flags {
                cf: true,
                pf: true,
                af: true,
                zf: true,
                sf: false,
                of: false,
            },
        ),
        (
            "pending word ADD",
            6,
            0xffff,
            1,
            Flags {
                cf: true,
                pf: true,
                af: true,
                zf: true,
                sf: false,
                of: false,
            },
        ),
        (
            "pending dword ADD",
            10,
            0x7fff_ffff,
            1,
            Flags {
                cf: false,
                pf: true,
                af: true,
                zf: false,
                sf: true,
                of: true,
            },
        ),
        (
            "pending byte SUB",
            1,
            0,
            1,
            Flags {
                cf: true,
                pf: true,
                af: true,
                zf: false,
                sf: true,
                of: false,
            },
        ),
        (
            "pending word SUB",
            5,
            0x8000,
            1,
            Flags {
                cf: false,
                pf: true,
                af: true,
                zf: false,
                sf: false,
                of: true,
            },
        ),
        (
            "pending dword SUB",
            9,
            0,
            1,
            Flags {
                cf: true,
                pf: true,
                af: true,
                zf: false,
                sf: true,
                of: false,
            },
        ),
        (
            "pending byte logical",
            3,
            0x80,
            0xdead_beef,
            Flags {
                cf: false,
                pf: false,
                af: false,
                zf: false,
                sf: true,
                of: false,
            },
        ),
        (
            "pending word logical",
            7,
            0x8000,
            0xdead_beef,
            Flags {
                cf: false,
                pf: true,
                af: false,
                zf: false,
                sf: true,
                of: false,
            },
        ),
        (
            "pending dword logical",
            11,
            0,
            0xdead_beef,
            Flags {
                cf: false,
                pf: true,
                af: false,
                zf: true,
                sf: false,
                of: false,
            },
        ),
    ] {
        records.push((
            name,
            StoredFlags {
                status_source: StoredStatusSource {
                    kind,
                    left,
                    right,
                    ..base.status_source
                },
                ..base
            },
            flags,
        ));
    }
    records
}

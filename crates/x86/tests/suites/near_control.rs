use wasm86_x86::{FlagBytes, StoredStatusSource};
#[path = "near_control/decoding.rs"]
mod decoding;
#[path = "near_control/faults.rs"]
mod faults;
#[path = "near_control/linkage.rs"]
mod linkage;
#[path = "near_control/memory.rs"]
mod memory;
#[path = "near_control/sequences.rs"]
mod sequences;

use crate::support::cases::{
    test_cases,
    FlagExpectation::Preserved,
    Flags, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::{
    CpuState,
    Gpr32::{self, *},
    StoredFlags,
};

#[rustfmt::skip]
fn relative_call_cases() -> Vec<Case> {
    [
        (0x1000, &[0xe8, 0, 0, 0, 0][..], 0x1005, 0x9000, &[5, 0x10, 0, 0][..]),
        (0x1000, &[0xe8, 0xff, 0xff, 0xff, 0x7f], 0x8000_1004, 0x9000, &[5, 0x10, 0, 0]),
        (0x1000, &[0xe8, 0, 0, 0, 0x80], 0x8000_1005, 0x9000, &[5, 0x10, 0, 0]),
        (0x1000, &[0xe8, 0xfb, 0xff, 0xff, 0xff], 0x1000, 0x9000, &[5, 0x10, 0, 0]),
        (0, &[0xe8, 0xfa, 0xff, 0xff, 0xff], 0xffff_ffff, 0x9000, &[5, 0, 0, 0]),
        (0xffff_fffc, &[0xe8, 0, 0, 0, 0], 1, 0x9000, &[1, 0, 0, 0]),
        (0x1234_1000, &[0x66, 0xe8, 0, 0], 0x1004, 0x9002, &[4, 0x10]),
        (0x1234_1000, &[0x66, 0xe8, 0xff, 0x7f], 0x9003, 0x9002, &[4, 0x10]),
        (0x1234_1000, &[0x66, 0xe8, 0, 0x80], 0x9004, 0x9002, &[4, 0x10]),
        (0x1234_fffe, &[0x66, 0xe8, 0xfd, 0xff], 0xffff, 0x9002, &[2, 0]),
        (0xffff_fffd, &[0x66, 0xe8, 0xfd, 0xff], 0xfffe, 0x9002, &[1, 0]),
        (0x1234_1000, &[0x66, 0x66, 0xe8, 0xfb, 0xff], 0x1000, 0x9002, &[5, 0x10]),
    ].into_iter().map(|(origin, code, target, stack, returned)| {
        Case::preserving_flags(format!("relative CALL {code:02x?} at {origin:08x}"), code)
            .at(origin).register(Esp, 0x9004, stack).dispatch(target)
            .memory(0x8fff, &[0xa5; 10], ReadWrite).expect_memory(stack, returned)
    }).collect()
}

struct TargetRegister {
    name: Gpr32,
    code: u8,
    value: u32,
    word: u32,
}

#[rustfmt::skip]
const TARGET_REGISTERS: [TargetRegister; 8] = [
    TargetRegister { name: Eax, code: 0, value: 0xa123_4567, word: 0x4567 },
    TargetRegister { name: Ecx, code: 1, value: 0xb234_8000, word: 0x8000 },
    TargetRegister { name: Edx, code: 2, value: 0xc345_ffff, word: 0xffff },
    TargetRegister { name: Ebx, code: 3, value: 0xd456_0000, word: 0 },
    TargetRegister { name: Esp, code: 4, value: 0x1234_9004, word: 0x9004 },
    TargetRegister { name: Ebp, code: 5, value: 0xe567_0123, word: 0x0123 },
    TargetRegister { name: Esi, code: 6, value: 0xf678_89ab, word: 0x89ab },
    TargetRegister { name: Edi, code: 7, value: 0x8765_fedc, word: 0xfedc },
];

#[rustfmt::skip]
fn register_target_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for register in &TARGET_REGISTERS {
        for (prefix, target, stack, returned) in [
            (&[][..], register.value, 0x1234_9000, &[2, 0x10, 0, 0][..]),
            (&[0x66][..], register.word, 0x1234_9002, &[3, 0x10][..]),
        ] {
            for (name, selector) in [("CALL", 0xd0), ("JMP", 0xe0)] {
                let code = [prefix, &[0xff, selector | register.code]].concat();
                let mut case = Case::preserving_flags(format!("{name} {:?} via {code:02x?}", register.name), &code)
                    .initial_register(Esp, 0x1234_9004).dispatch(target);
                if register.name != Esp { case = case.initial_register(register.name, register.value); }
                if name == "CALL" {
                    case = case.expect_register(Esp, crate::support::cases::RegisterExpectation::Exact(stack))
                        .memory(0x1234_8fff, &[0xa5; 10], ReadWrite).expect_memory(stack, returned);
                }
                cases.push(case);
            }
        }
    }
    cases
}

#[rustfmt::skip]
fn return_cases() -> Vec<Case> {
    [
        (&[0xc3][..], 0x9004, 0xf123_8001),
        (&[0x66, 0xc3], 0x9002, 0x8001),
        (&[0xc2, 0, 0], 0x9004, 0xf123_8001),
        (&[0x66, 0xc2, 0, 0], 0x9002, 0x8001),
        (&[0xc2, 0xff, 0x7f], 0x0001_1003, 0xf123_8001),
        (&[0x66, 0xc2, 0xff, 0x7f], 0x0001_1001, 0x8001),
        (&[0xc2, 0, 0x80], 0x0001_1004, 0xf123_8001),
        (&[0x66, 0xc2, 0, 0x80], 0x0001_1002, 0x8001),
        (&[0xc2, 0xff, 0xff], 0x0001_9003, 0xf123_8001),
        (&[0x66, 0xc2, 0xff, 0xff], 0x0001_9001, 0x8001),
    ].into_iter().map(|(code, stack, target)| {
        Case::preserving_flags(format!("RET width and unsigned cleanup: {code:02x?}"), code)
            .at(0x1234_1000).register(Esp, 0x9000, stack).dispatch(target)
            .memory(0x8fff, &[0xa5, 1, 0x80, 0x23, 0xf1, 0x5a], ReadOnly)
    }).collect()
}

fn flag_cases() -> Vec<Case> {
    let flags = Flags {
        cf: true,
        pf: true,
        af: true,
        zf: false,
        sf: true,
        of: false,
    };
    let pending = StoredFlags {
        status_source: StoredStatusSource {
            kind: 9,
            left: 7,
            right: 8,
            ..(CpuState::filled(0x5a).flags).status_source
        },
        bytes: FlagBytes {
            cf: 0,
            pf: 0,
            af: 0,
            zf: 1,
            sf: 0,
            of: 1,
            ..(CpuState::filled(0x5a).flags).bytes
        },
    };
    let mut cases = Vec::new();
    for stored in [None, Some(pending)] {
        for (code, target, stack, returned) in [
            (
                &[0xe8, 0x7f, 0, 0, 0][..],
                0x1084,
                0x8ffc,
                &[5, 0x10, 0, 0][..],
            ),
            (&[0x66, 0xff, 0xd0], 0x8001, 0x8ffe, &[3, 0x10][..]),
            (&[0x66, 0xff, 0xe0], 0x8001, 0x9000, &[][..]),
            (&[0x66, 0xc3], 0x5678, 0x9002, &[][..]),
            (&[0xc2, 0xff, 0xff], 0x1234_5678, 0x0001_9003, &[][..]),
        ] {
            let mut case = Case::new(
                format!(
                    "near transfer {code:02x?}, pending flags {}",
                    stored.is_some()
                ),
                code,
                flags,
                Flags::all(Preserved),
            )
            .preserve_flag_record()
            .initial_register(Eax, 0xffff_8001)
            .register(Esp, 0x9000, stack)
            .dispatch(target)
            .memory(
                0x8ffb,
                &[0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0x78, 0x56, 0x34, 0x12, 0x5a],
                ReadWrite,
            );
            if !returned.is_empty() {
                case = case.expect_memory(stack, returned);
            }
            if let Some(record) = stored {
                case = case.stored_flags(record);
            }
            cases.push(case);
        }
    }
    cases
}

test_cases!(
    relative_call_targets_and_return_addresses,
    relative_call_cases()
);
test_cases!(
    register_targets_use_original_values,
    register_target_cases()
);
test_cases!(return_width_and_unsigned_cleanup, return_cases());
test_cases!(concrete_and_pending_flags_are_preserved, flag_cases());

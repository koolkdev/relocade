use wasm86_x86::Gpr32::{Eax, Ebx};

use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Set, Undefined},
    Flags, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};

#[rustfmt::skip]
fn ordinary_memory() -> Vec<Case> {
    vec![
        Case::new("ADD EAX,[EBX] reads a read-only source", &[0x03, 0x03], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x10, 0).initial_register(Ebx, 0x4000)
            .memory(0x4000, &[0xf0, 0xff, 0xff, 0xff], ReadOnly),
        Case::new("SUB EAX,[EBX] uses memory as its right operand", &[0x2b, 0x03], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Set })
            .register(Eax, 1, 0x8000_0001).initial_register(Ebx, 0x4000)
            .memory(0x4000, &[0, 0, 0, 0x80], ReadOnly),
        Case::new("CMP [EBX],EAX needs no write permission", &[0x39, 0x03], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Clear, of: Set })
            .initial_register(Eax, 1).initial_register(Ebx, 0x4000)
            .memory(0x4000, &[0, 0, 0, 0x80], ReadOnly),
        Case::new("CMP EAX,[EBX] keeps operand order", &[0x3b, 0x03], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Set })
            .initial_register(Eax, 1).initial_register(Ebx, 0x4000)
            .memory(0x4000, &[0, 0, 0, 0x80], ReadOnly),
        Case::new("ADD EAX,[EAX] reads the old address register", &[0x03, 0x00], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4000, 0x4005).initial_register(Ebx, 0x4000)
            .memory(0x4000, &[5, 0, 0, 0], ReadOnly),
        Case::new("ADD [EAX],AL uses original address AL", &[0x00, 0x00], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4020).initial_register(Ebx, 0x4020)
            .memory(0x401f, &[0xa5, 0xe0, 0xcc, 0x5a], ReadWrite)
            .expect_memory(0x4020, &[0]),
        Case::new("ADD word [EBX-128],AX wraps only the word", &[0x66, 0x01, 0x43, 0x80], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 1).initial_register(Ebx, 0x40a0)
            .memory(0x401f, &[0xa5, 0xff, 0xff, 0x5a], ReadWrite)
            .expect_memory(0x4020, &[0, 0]),
        Case::new("SUB byte [EBX],AL borrows and truncates its store", &[0x28, 0x03], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
            .initial_register(Eax, 1).initial_register(Ebx, 0x4020)
            .memory(0x401f, &[0xa5, 0, 0xcc, 0x5a], ReadWrite)
            .expect_memory(0x4020, &[0xff]),
        Case::new("SUB word [EBX-128],AX preserves the store width", &[0x66, 0x29, 0x43, 0x80], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Clear, of: Set })
            .initial_register(Eax, 1).initial_register(Ebx, 0x40a0)
            .memory(0x401f, &[0xa5, 0, 0x80, 0x5a], ReadWrite)
            .expect_memory(0x4020, &[0xff, 0x7f]),
        Case::new("SUB [EBX],EAX keeps memory as the left operand", &[0x29, 0x03], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Clear, of: Set })
            .initial_register(Eax, 1).initial_register(Ebx, 0x4020)
            .memory(0x401f, &[0xa5, 0, 0, 0, 0x80], ReadWrite)
            .expect_memory(0x4020, &[0xff, 0xff, 0xff, 0x7f]),
        Case::new("ADD byte [EBX],80 uses the group immediate", &[0x80, 0x03, 0x80], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set })
            .initial_register(Eax, 1).initial_register(Ebx, 0x4020)
            .memory(0x401f, &[0xa5, 0x80, 0xcc, 0x5a], ReadWrite)
            .expect_memory(0x4020, &[0]),
    ]
}

test_cases!(ordinary_memory_operands, ordinary_memory());

#[rustfmt::skip]
fn page_spans() -> Vec<Case> {
    let mut cases = Vec::new();
    for (layout, next_frame) in [("contiguous", 0x9000), ("scattered", 0xa000)] {
        cases.push(Case::new(format!("{layout} dword ADD keeps adjacent canaries"), &[0x01, 0x03], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
            .initial_register(Eax, 1).initial_register(Ebx, 0x4ffe)
            .map_page(4, 0x8000, ReadWrite).map_page(5, next_frame, ReadWrite)
            .backing(0x8ffd, &[0xa5, 0xff, 0xff]).backing(next_frame, &[0xff, 0x7f, 0x5a])
            .expect_memory(0x4ffe, &[0, 0, 0, 0x80]));
        for (name, code, eax, before, after, flags) in [
            ("byte TEST reads AH and readonly memory", &[0x84, 0x23][..], 0x4433_8001,
                &[0xf0][..], None,
                Flags { cf: Clear, pf: Clear, af: Undefined, zf: Clear, sf: Set, of: Clear }),
            ("word immediate TEST reads a readonly page span", &[0x66, 0xf7, 0x03, 0x00, 0xff][..], 0x4433_8001,
                &[0xff, 0x80][..], None,
                Flags { cf: Clear, pf: Set, af: Undefined, zf: Clear, sf: Set, of: Clear }),
            ("dword TEST preserves memory and its source register", &[0x85, 0x03][..], 0xffff_00ff,
                &[1, 0, 0, 0x80][..], None,
                Flags { cf: Clear, pf: Clear, af: Undefined, zf: Clear, sf: Set, of: Clear }),
            ("byte AND updates memory using old AH", &[0x20, 0x23][..], 0x4433_8001,
                &[0xf3][..], Some(&[0x80][..]),
                Flags { cf: Clear, pf: Clear, af: Undefined, zf: Clear, sf: Set, of: Clear }),
            ("word OR updates exactly two bytes", &[0x66, 0x09, 0x03][..], 0x4433_0001,
                &[0, 0x80][..], Some(&[1, 0x80][..]),
                Flags { cf: Clear, pf: Clear, af: Undefined, zf: Clear, sf: Set, of: Clear }),
            ("dword XOR updates the whole proven page span", &[0x31, 0x03][..], 0x8000_0000,
                &[0xff, 0xff, 0xff, 0xff][..], Some(&[0xff, 0xff, 0xff, 0x7f][..]),
                Flags { cf: Clear, pf: Set, af: Undefined, zf: Clear, sf: Clear, of: Clear }),
        ] {
            let permissions = if after.is_some() { ReadWrite } else { ReadOnly };
            let mut case = Case::new(format!("{layout} {name}"), code, Flags::all(true), flags)
                .initial_register(Eax, eax).initial_register(Ebx, 0x4fff)
                .map_page(4, 0x8000, permissions).map_page(5, next_frame, permissions)
                .backing(0x8ffe, &[0xa5, before[0]]).backing(next_frame, &before[1..])
                .backing(next_frame + before.len() as u32 - 1, &[0x5a]);
            if let Some(bytes) = after { case = case.expect_memory(0x4fff, bytes); }
            cases.push(case);
        }
    }
    cases
}

test_cases!(contiguous_and_scattered_memory, page_spans());

use wasm86_x86::{
    CpuState,
    Gpr32::{Eax, Ebx, Ecx},
    StatusFlags, StoredFlags,
};

use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Preserved, Set},
    Flags, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};

#[rustfmt::skip]
fn write_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, width, address, first, second, fault_address, error) in [
        ("missing byte page", 8, 0x4020, None, None, 0x4020, 2),
        ("read-only unequal dword", 32, 0x4020, Some(ReadOnly), None, 0x4020, 3),
        ("read-only second word page", 16, 0x4fff, Some(ReadWrite), Some(ReadOnly), 0x5000, 3),
        ("missing second dword page", 32, 0x4ffe, Some(ReadWrite), None, 0x5000, 2),
        ("word range cannot wrap", 16, 0xffff_ffff, Some(ReadWrite), None, 0xffff_ffff, 2),
        ("dword range cannot wrap", 32, 0xffff_fffd, Some(ReadWrite), None, 0xffff_fffd, 2),
    ] {
        for (operation, opcode) in [("XADD", 0xc0), ("CMPXCHG", 0xb0)] {
            let mut code = Vec::new();
            if width == 16 { code.push(0x66); }
            code.extend_from_slice(&[0x0f, opcode + u8::from(width != 8), 0x03]);
            let mut case = Case::new(format!("{operation}: {name}"), &code,
                Flags { cf: true, pf: true, af: false, zf: false, sf: true, of: true }, Flags::all(Preserved))
                .stored_flags(StoredFlags {
                    kind: 9, left: 0x7fff_fffe, right: 0xffff_fffe,
                    status: StatusFlags { cf: 1, pf: 1, af: 1, zf: 1, sf: 1, of: 1 },
                    non_status: [0, 1, 0, 0, 0, 0xa5],
                    ..CpuState::filled(0xa5).flags
                }).preserve_flag_record()
                .initial_register(Eax, 0x4433_2211).initial_register(Ebx, address)
                .backing(0x801f, &[0x5a, 0x80, 0xfe, 0xdc, 0xba, 0x5a])
                .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
                .fault(fault_address, error);
            if let Some(permissions) = first { case = case.map_page(address >> 12, 0x8000, permissions); }
            if let Some(permissions) = second { case = case.map_page(5, 0xa000, permissions); }
            cases.push(case);
        }
    }
    cases
}

#[rustfmt::skip]
fn scattered_memory() -> Vec<Case> {
    vec![
        Case::new("cross-page XADD reads the old dword and writes its sum", &[0x0f, 0xc1, 0x03], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 1, 0xffff_ffff).initial_register(Ebx, 0x4ffe)
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .backing(0x8ffd, &[0x5a, 0xff, 0xff]).backing(0xa000, &[0xff, 0xff, 0x5a])
            .expect_memory(0x4ffe, &[0, 0, 0, 0]),
        Case::new("cross-page CMPXCHG retains its equal accumulator and stores old CX", &[0x66, 0x0f, 0xb1, 0x0b], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4433_ff80).initial_register(Ebx, 0x4fff).initial_register(Ecx, 0x8877_6655)
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x80]).backing(0xa000, &[0xff, 0x5a])
            .expect_memory(0x4fff, &[0x55, 0x66]),
    ]
}

test_cases!(access_faults_preserve_incoming_recipe, write_faults());
test_cases!(scattered_writes_keep_canaries, scattered_memory());

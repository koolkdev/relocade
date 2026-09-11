use wasm86_x86::{Gpr32, StatusFlags, StoredFlags};

use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Preserved, Set},
    Flags, InstructionCase as Case,
};

const INITIAL: Flags<bool> = Flags::all(true);
const INVALID: StoredFlags = StoredFlags {
    kind: 0xff,
    reserved: [0xa5; 3],
    left: 0xa5a5_a5a5,
    right: 0xa5a5_a5a5,
    status: StatusFlags {
        cf: 1,
        pf: 0xa5,
        af: 0xa5,
        zf: 0xa5,
        sf: 0xa5,
        of: 0xa5,
    },
    non_status: [0xa5; 6],
};

// Register enumeration selects an encoding; all expected values remain literal.
#[rustfmt::skip]
fn compact_registers() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, opcode, inputs, outputs, flags) in [
        ("INC", 0x40, [0xffff_ffff, 0x9234_ffff], [0, 0x9234_0000],
            Flags { cf: Preserved, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }),
        ("DEC", 0x48, [0, 0x9234_0000], [0xffff_ffff, 0x9234_ffff],
            Flags { cf: Preserved, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }),
    ] {
        for (width, (operand, prefix)) in [("dword", vec![]), ("word", vec![0x66])].into_iter().enumerate() {
            for (index, register) in Gpr32::ALL.into_iter().enumerate() {
                let mut code = prefix.clone();
                code.push(opcode + index as u8);
                cases.push(Case::new(format!("compact {name} {register:?} {operand}"), &code, INITIAL, flags)
                    .register(register, inputs[width], outputs[width]));
            }
        }
    }
    cases
}

#[rustfmt::skip]
fn modrm_registers() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, opcode, extension, output, flags) in [
        ("INC", 0xff, 0x00, 2,
            Flags { cf: Preserved, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear }),
        ("DEC", 0xff, 0x08, 0,
            Flags { cf: Preserved, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear }),
        ("NOT", 0xf7, 0x10, 0xffff_fffe, Flags::all(Preserved)),
        ("NEG", 0xf7, 0x18, 0xffff_ffff,
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }),
    ] {
        for (index, register) in Gpr32::ALL.into_iter().enumerate() {
            let name = format!("ModRM {name} {register:?}");
            let code = [opcode, 0xc0 | extension | index as u8];
            let case = match extension {
                0x10 => Case::preserving_flags(name, &code).stored_flags(INVALID),
                0x18 => Case::replacing_flags(name, &code, flags).stored_flags(INVALID),
                _ => Case::new(name, &code, INITIAL, flags),
            };
            cases.push(case.register(register, 1, output));
        }
    }
    cases
}

#[rustfmt::skip]
fn byte_aliases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, opcode, extension, outputs, flags) in [
        ("INC", 0xfe, 0x00, [0x9234_8081, 0x9234_8180],
            Flags { cf: Preserved, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear }),
        ("DEC", 0xfe, 0x08, [0x9234_807f, 0x9234_7f80],
            Flags { cf: Preserved, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Set }),
        ("NOT", 0xf6, 0x10, [0x9234_807f, 0x9234_7f80], Flags::all(Preserved)),
        ("NEG", 0xf6, 0x18, [0x9234_8080, 0x9234_8080],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Set }),
    ] {
        for (index, register) in [Gpr32::Eax, Gpr32::Ecx, Gpr32::Edx, Gpr32::Ebx].into_iter().cycle().take(8).enumerate() {
            let name = format!("byte {name} alias {index} ({register:?})");
            let code = [opcode, 0xc0 | extension | index as u8];
            let case = match extension {
                0x10 => Case::preserving_flags(name, &code).stored_flags(INVALID),
                0x18 => Case::replacing_flags(name, &code, flags).stored_flags(INVALID),
                _ => Case::new(name, &code, INITIAL, flags),
            };
            cases.push(case.register(register, 0x9234_8080, outputs[index / 4]));
        }
    }
    cases
}

test_cases!(compact_inc_dec_registers, compact_registers());
test_cases!(modrm_selects_each_register, modrm_registers());
test_cases!(byte_forms_select_each_alias, byte_aliases());

use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Edx};

use crate::support::{
    cases::{
        FlagExpectation::{Clear, Preserved, Set},
        Flags,
    },
    sequences::{Checkpoint, SequenceCase as Case},
};

use super::super::STORED_FLAGS;

const INITIAL: Flags<bool> = Flags {
    cf: true,
    pf: true,
    af: false,
    zf: false,
    sf: true,
    of: true,
};

#[rustfmt::skip]
pub(super) fn flag_dependencies() -> Vec<Case> {
    vec![
        Case::from_opaque_flags("zero ROL keeps pending ADD overflow")
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Eax, 0x4433_817f), (Ecx, 0x8877_6620), (Edx, 0xccbb_aa01), (Ebx, 0x10ff_eedd)])
            .step(Checkpoint::new(&[0x00, 0xd0],
                Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Set, of: Set }) // ADD AL,DL
                .register(Eax, 0x4433_8180))
            .step(Checkpoint::preserving_flags(&[0xd2, 0xc4])) // ROL AH,CL
            .step(Checkpoint::preserving_flags(&[0x0f, 0x90, 0xc3]) // SETO BL
                .register(Ebx, 0x10ff_ee01)),
        Case::from_opaque_flags("zero ROR keeps pending ADD overflow")
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Eax, 0x4433_817f), (Ecx, 0x8877_6620), (Edx, 0xccbb_aa01), (Ebx, 0x10ff_eedd)])
            .step(Checkpoint::new(&[0x00, 0xd0],
                Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Set, of: Set }) // ADD AL,DL
                .register(Eax, 0x4433_8180))
            .step(Checkpoint::preserving_flags(&[0xd2, 0xcc])) // ROR AH,CL
            .step(Checkpoint::preserving_flags(&[0x0f, 0x90, 0xc3]) // SETO BL
                .register(Ebx, 0x10ff_ee01)),
        Case::new("ROL AL,0 carry reaches SBB and SETC", INITIAL)
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Eax, 0x4433_2280), (Edx, 0), (Ebx, 0x10ff_eedd)])
            .step(Checkpoint::preserving_flags(&[0xc0, 0xc0, 0x00])
                .register(Eax, 0x4433_2280))
            .step(Checkpoint::new(&[0x83, 0xda, 0x00],
                Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }) // SBB EDX,0
                .register(Edx, 0xffff_ffff))
            .step(Checkpoint::preserving_flags(&[0x0f, 0x92, 0xc3]) // SETC BL
                .register(Ebx, 0x10ff_ee01)),
        Case::new("ROL AL,1 carry reaches SBB and SETC", INITIAL)
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Eax, 0x4433_2280), (Edx, 0), (Ebx, 0x10ff_eedd)])
            .step(Checkpoint::new(&[0xc0, 0xc0, 0x01],
                Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
                .register(Eax, 0x4433_2201))
            .step(Checkpoint::new(&[0x83, 0xda, 0x00],
                Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }) // SBB EDX,0
                .register(Edx, 0xffff_ffff))
            .step(Checkpoint::preserving_flags(&[0x0f, 0x92, 0xc3]) // SETC BL
                .register(Ebx, 0x10ff_ee01)),
        Case::new("ROL AL,8 carry reaches SBB and SETC", INITIAL)
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Eax, 0x4433_2280), (Edx, 0), (Ebx, 0x10ff_eedd)])
            .step(Checkpoint::new(&[0xc0, 0xc0, 0x08],
                Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
                .register(Eax, 0x4433_2280))
            .step(Checkpoint::new(&[0x83, 0xda, 0x00],
                Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear }) // SBB EDX,0
                .register(Edx, 0))
            .step(Checkpoint::preserving_flags(&[0x0f, 0x92, 0xc3]) // SETC BL
                .register(Ebx, 0x10ff_ee00)),
        Case::new("ROR AL,0 carry reaches SBB and SETC", INITIAL)
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Eax, 0x4433_2201), (Edx, 0), (Ebx, 0x10ff_eedd)])
            .step(Checkpoint::preserving_flags(&[0xc0, 0xc8, 0x00])
                .register(Eax, 0x4433_2201))
            .step(Checkpoint::new(&[0x83, 0xda, 0x00],
                Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }) // SBB EDX,0
                .register(Edx, 0xffff_ffff))
            .step(Checkpoint::preserving_flags(&[0x0f, 0x92, 0xc3]) // SETC BL
                .register(Ebx, 0x10ff_ee01)),
        Case::new("ROR AL,1 carry reaches SBB and SETC", INITIAL)
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Eax, 0x4433_2201), (Edx, 0), (Ebx, 0x10ff_eedd)])
            .step(Checkpoint::new(&[0xc0, 0xc8, 0x01],
                Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
                .register(Eax, 0x4433_2280))
            .step(Checkpoint::new(&[0x83, 0xda, 0x00],
                Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }) // SBB EDX,0
                .register(Edx, 0xffff_ffff))
            .step(Checkpoint::preserving_flags(&[0x0f, 0x92, 0xc3]) // SETC BL
                .register(Ebx, 0x10ff_ee01)),
        Case::new("ROR AL,8 carry reaches SBB and SETC", INITIAL)
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Eax, 0x4433_2201), (Edx, 0), (Ebx, 0x10ff_eedd)])
            .step(Checkpoint::new(&[0xc0, 0xc8, 0x08],
                Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
                .register(Eax, 0x4433_2201))
            .step(Checkpoint::new(&[0x83, 0xda, 0x00],
                Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear }) // SBB EDX,0
                .register(Edx, 0))
            .step(Checkpoint::preserving_flags(&[0x0f, 0x92, 0xc3]) // SETC BL
                .register(Ebx, 0x10ff_ee00)),
    ]
}

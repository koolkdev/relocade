use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Edx};

use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set},
        Flags,
    },
    sequences::{Checkpoint, SequenceCase as Case},
};

use super::super::STORED_FLAGS;

#[rustfmt::skip]
pub(super) fn flag_dependencies() -> Vec<Case> {
    vec![
        Case::from_opaque_flags("zero SHL keeps pending ADD overflow")
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Eax, 0x4433_017f), (Ecx, 0x8877_6620), (Edx, 0xccbb_aa01), (Ebx, 0x10ff_eedd)])
            .step(Checkpoint::new(&[0x00, 0xd0],
                Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Set, of: Set }) // ADD AL,DL
                .register(Eax, 0x4433_0180))
            .step(Checkpoint::preserving_flags(&[0xd2, 0xe4])) // SHL AH,CL
            .step(Checkpoint::preserving_flags(&[0x0f, 0x90, 0xc3]) // SETO BL
                .register(Ebx, 0x10ff_ee01)),
        Case::from_opaque_flags("changing CL keeps the latest nonzero shift flags")
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Eax, 0x4433_8111), (Ecx, 0x8877_6601), (Edx, 0xccbb_aa99)])
            .step(Checkpoint::new(&[0xd2, 0xe1], Flags::all(Clear)) // SHL CL,CL
                .register(Ecx, 0x8877_6602))
            .step(Checkpoint::new(&[0xd2, 0xe4], Flags::all(Clear)) // SHL AH,CL
                .register(Eax, 0x4433_0411))
            .step(Checkpoint::new(&[0xd2, 0xe9],
                Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear }) // SHR CL,CL
                .register(Ecx, 0x8877_6600))
            .step(Checkpoint::preserving_flags(&[0xd3, 0xf8])) // SAR EAX,CL
            .step(Checkpoint::preserving_flags(&[0x0f, 0x94, 0xc2]) // SETZ DL
                .register(Edx, 0xccbb_aa01)),
    ]
}

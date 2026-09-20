use super::{pushed, INPUT};
use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set},
        Flags,
        Permissions::ReadWrite,
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::Gpr32::*;

fn sequences() -> Vec<Case> {
    let mut cases = Vec::new();
    for (width, prefix, restored_eax) in [(2, &[0x66][..], 0x7654_2222), (4, &[][..], 0x1111_2222)]
    {
        let end = 0x9000 - 8 * width as u32;
        cases.push(
            Case::preserving_flags(format!(
                "PUSHA/POPA with intervening full register writes, width={width}"
            ))
            .initial_registers(&INPUT)
            .initial_register(Esp, 0x9000)
            .memory(end, &vec![0xa5; width * 8], ReadWrite)
            .step(
                Step::preserving_flags(&[prefix, &[0x60]].concat())
                    .register(Esp, end)
                    .expect_memory(end, &pushed(0x9000, width)),
            )
            .step(
                Step::preserving_flags(&[0xb8, 0x10, 0x32, 0x54, 0x76]).register(Eax, 0x7654_3210),
            )
            .step(
                Step::preserving_flags(&[prefix, &[0x61]].concat())
                    .register(Esp, 0x9000)
                    .register(Eax, restored_eax),
            )
            .step(Step::preserving_flags(&[0x89, 0xc1]).register(Ecx, restored_eax))
            .step(Step::preserving_flags(&[0x89, 0xe2]).register(Edx, 0x9000))
            .step(Step::preserving_flags(&[0x8b, 0x1d, 0, 0x60, 0, 0]).fault(0x6000, 0))
            .trailing_code(&[0x89, 0xc7], 1),
        );
    }
    cases.push(
        Case::from_opaque_flags(
            "PUSHA fault publishes earlier arithmetic and keeps its completed stores",
        )
        .initial_registers(&INPUT)
        .initial_register(Esp, 0x9000)
        .memory(0x5000, &[0xa5; 4], ReadWrite)
        .step(Step::preserving_flags(&[0xb8, 0xff, 0xff, 0xff, 0x7f]).register(Eax, 0x7fff_ffff))
        .step(
            Step::new(
                &[0x83, 0xc0, 1],
                Flags {
                    cf: Clear,
                    pf: Set,
                    af: Set,
                    zf: Clear,
                    sf: Set,
                    of: Set,
                },
            )
            .register(Eax, 0x8000_0000),
        )
        .step(Step::preserving_flags(&[0xbc, 4, 0x50, 0, 0]).register(Esp, 0x5004))
        .step(
            Step::preserving_flags(&[0x60])
                .expect_memory(0x5000, &[0, 0, 0, 0x80])
                .fault(0x4ffc, 2),
        )
        .trailing_code(&[0x89, 0xc7], 1),
    );
    for (width, prefix, start, edi, esi, ebp) in [
        (
            2,
            &[0x66][..],
            0x4ffau32,
            0x7654_0304,
            0xbbbb_1314,
            0x9999_2324,
        ),
        (4, &[][..], 0x4ff4, 0x0102_0304, 0x1112_1314, 0x2122_2324),
    ] {
        cases.push(Case::from_opaque_flags(format!(
            "POPA fault combines partial restores with preceding aliases and arithmetic, width={width}",
        ))
        .initial_registers(&INPUT).initial_register(Esp, 0x9000)
        .memory(start, &super::pop_source(width)[..3 * width], ReadWrite)
        .step(Step::preserving_flags(&[0xbf, 0x10, 0x32, 0x54, 0x76]).register(Edi, 0x7654_3210))
        .step(Step::preserving_flags(&[0x66, 0xbe, 0xef, 0xbe]).register(Esi, 0xbbbb_beef))
        .step(Step::preserving_flags(&[0xb8, 0xff, 0xff, 0xff, 0x7f]).register(Eax, 0x7fff_ffff))
        .step(Step::new(&[0x83, 0xc0, 1], Flags {
            cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set,
        }).register(Eax, 0x8000_0000))
        .step(Step::preserving_flags(&[&[0xbc][..], &start.to_le_bytes()].concat()).register(Esp, start))
        .step(Step::preserving_flags(&[prefix, &[0x61]].concat())
            .register(Edi, edi).register(Esi, esi).register(Ebp, ebp).fault(0x5000, 0))
        .trailing_code(&[0x89, 0xf8], 1));
    }
    cases
}

test_sequences!(register_restore_and_fault_progress, sequences());

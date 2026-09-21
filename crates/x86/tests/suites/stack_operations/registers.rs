use super::{code16, stack_segment};
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
        RegisterExpectation::Exact,
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::{
    Gpr32::{self, *},
    Segment, SegmentAttributes, StoredSegment,
};

const INPUT: [(Gpr32, u32); 7] = [
    (Eax, 0x1111_2222),
    (Ecx, 0x3333_4444),
    (Edx, 0x5555_6666),
    (Ebx, 0x7777_8888),
    (Ebp, 0x9999_aaaa),
    (Esi, 0xbbbb_cccc),
    (Edi, 0xdddd_eeee),
];

// Slot index, destination and value. Slot 3 never restores ESP.
const RESTORED: [(usize, Gpr32, u32); 7] = [
    (0, Edi, 0x0102_0304),
    (1, Esi, 0x1112_1314),
    (2, Ebp, 0x2122_2324),
    (4, Ebx, 0x4142_4344),
    (5, Edx, 0x5152_5354),
    (6, Ecx, 0x6162_6364),
    (7, Eax, 0x7172_7374),
];

fn slot_bytes(values: &[u32], width: usize) -> Vec<u8> {
    values
        .iter()
        .flat_map(|v| v.to_le_bytes()[..width].to_vec())
        .collect()
}

fn pushed(esp: u32, width: usize) -> Vec<u8> {
    slot_bytes(
        &[
            0xdddd_eeee,
            0xbbbb_cccc,
            0x9999_aaaa,
            esp,
            0x7777_8888,
            0x5555_6666,
            0x3333_4444,
            0x1111_2222,
        ],
        width,
    )
}

fn pop_source(width: usize) -> Vec<u8> {
    slot_bytes(
        &[
            0x0102_0304,
            0x1112_1314,
            0x2122_2324,
            0xdead_beef,
            0x4142_4344,
            0x5152_5354,
            0x6162_6364,
            0x7172_7374,
        ],
        width,
    )
}

fn expect_restored(mut case: Case, word: bool, completed_slots: usize) -> Case {
    for (slot, register, value) in RESTORED {
        if slot >= completed_slots {
            continue;
        }
        let (_, input) = INPUT.iter().find(|(name, _)| *name == register).unwrap();
        case = case.expect_register(
            register,
            Exact(if word {
                (input & 0xffff_0000) | (value & 0xffff)
            } else {
                value
            }),
        );
    }
    case
}

#[rustfmt::skip]
fn transfers() -> Vec<Case> {
    let mut cases = Vec::new();
    for (width, push, pop) in [(2, &[0x66, 0x60][..], &[0x66, 0x61][..]), (4, &[0x60][..], &[0x61][..])] {
        let size = (8 * width) as u32;
        cases.push(Case::preserving_flags(format!("PUSHA slot order and saved entry ESP, width={width}"), push)
            .initial_registers(&INPUT).register(Esp, 0x9000, 0x9000 - size)
            .memory(0x9000 - size, &vec![0xa5; 8 * width], ReadWrite)
            .expect_memory(0x9000 - size, &pushed(0x9000, width)));
        cases.push(expect_restored(Case::preserving_flags(format!("POPA slot order and discarded ESP, width={width}"), pop)
            .initial_registers(&INPUT).register(Esp, 0x9000, 0x9000 + size)
            .memory(0x9000, &pop_source(width), ReadOnly), width == 2, 8));
    }
    let frame = pushed(0xabcd_0008, 4);
    let source = pop_source(4);
    cases.extend([
        Case::preserving_flags("PUSHAD wraps between slots on a small stack in 16-bit code", &[0x66, 0x60])
            .segmented_only().segment(Segment::Cs, code16())
            .segment(Segment::Ss, stack_segment(0, 0xffff, false))
            .initial_registers(&INPUT).register(Esp, 0xabcd_0008, 0xabcd_ffe8)
            .memory(0xffe8, &[0xa5; 24], ReadWrite).memory(0, &[0xa5; 8], ReadWrite)
            .expect_memory(0xffe8, &frame[..24]).expect_memory(0, &frame[24..]),
        expect_restored(Case::preserving_flags("POPAD wraps between slots on a small stack", &[0x61])
            .segmented_only().segment(Segment::Ss, stack_segment(0, 0xffff, false))
            .initial_registers(&INPUT).register(Esp, 0xabcd_fff8, 0xabcd_0018)
            .memory(0xfff8, &source[..8], ReadOnly).memory(0, &source[8..], ReadOnly)
            .memory(0x10000, &[0x5a; 32], ReadOnly), false, 8),
        Case::preserving_flags("PUSHAD keeps a straddling slot consecutive after wrapping", &[0x60])
            .segmented_only().segment(Segment::Ss, stack_segment(0, 0x2ffff, false))
            .initial_registers(&INPUT).register(Esp, 0xabcd_0002, 0xabcd_ffe2)
            .memory(0xffe2, &[0xa5; 32], ReadWrite).expect_memory(0xffe2, &pushed(0xabcd_0002, 4)),
        expect_restored(Case::preserving_flags("POPA keeps a straddling slot consecutive then wraps", &[0x66, 0x61])
            .segmented_only().segment(Segment::Ss, stack_segment(0, 0x2ffff, false))
            .initial_registers(&INPUT).register(Esp, 0xabcd_ffff, 0xabcd_000f)
            .memory(0xffff, &pop_source(2)[..2], ReadOnly).memory(1, &pop_source(2)[2..], ReadOnly), true, 8),
        expect_restored(Case::preserving_flags("POPAD permits a discarded slot above 64K within SS.limit", &[0x61])
            .segmented_only().segment(Segment::Ss, stack_segment(0, 0x2ffff, false))
            .initial_registers(&INPUT).register(Esp, 0xabcd_fff2, 0xabcd_0012)
            .memory(0xfff2, &source[..16], ReadOnly).memory(2, &source[16..], ReadOnly), false, 8),
    ]);
    cases
}

#[rustfmt::skip]
fn fault_progress() -> Vec<Case> {
    let mut cases = vec![
        Case::preserving_flags("PUSHA first split slot faults before storing bytes", &[0x66, 0x60])
            .initial_registers(&INPUT).initial_register(Esp, 0x5001)
            .memory(0x4fff, &[0xa5], ReadWrite).fault(0x5000, 2),
        Case::preserving_flags("PUSHAD retains four stores and entry ESP on a later fault", &[0x60])
            .initial_registers(&INPUT).initial_register(Esp, 0x5010)
            .memory(0x5000, &[0xa5; 16], ReadWrite).expect_memory(0x5000, &pushed(0x5010, 4)[16..])
            .fault(0x4ffc, 2),
        Case::preserving_flags("POPAD first split slot preserves entry state", &[0x61])
            .initial_registers(&INPUT).initial_register(Esp, 0x4fff)
            .memory(0x4fff, &[0xa5], ReadOnly).fault(0x5000, 0),
        expect_restored(Case::preserving_flags("POPA split second slot retains only the first restore", &[0x66, 0x61])
            .initial_registers(&INPUT).initial_register(Esp, 0x4ffd)
            .memory(0x4ffd, &pop_source(2)[..3], ReadOnly).fault(0x5000, 0), true, 1),
        expect_restored(Case::preserving_flags("POPAD pages the complete discarded slot across 64K", &[0x61])
            .segmented_only().segment(Segment::Ss, stack_segment(0, 0x2ffff, false))
            .initial_registers(&INPUT).initial_register(Esp, 0xabcd_fff3)
            .memory(0xfff3, &pop_source(4)[..13], ReadOnly).memory(3, &pop_source(4)[16..], ReadOnly)
            .fault(0x10000, 0), false, 3),
        expect_restored(Case::preserving_flags("POPA faults on the discarded slot after stack wrap", &[0x61])
            .segmented_only().segment(Segment::Cs, code16())
            .segment(Segment::Ss, stack_segment(0, 0xffff, false))
            .initial_registers(&INPUT).initial_register(Esp, 0xabcd_fffa)
            .memory(0xfffa, &pop_source(2)[..6], ReadOnly).fault(0, 0), true, 3),
        expect_restored(Case::preserving_flags("POPAD retains restores beyond the discarded slot on a later fault", &[0x61])
            .initial_registers(&INPUT).initial_register(Esp, 0x4fe4)
            .memory(0x4fe4, &pop_source(4)[..28], ReadOnly).fault(0x5000, 0), false, 7),
        expect_restored(Case::preserving_flags("POPA retains two restores on a later SS fault", &[0x66, 0x61])
            .segmented_only().segment(Segment::Ss, stack_segment(0x20000, 0x4fff, true))
            .initial_registers(&INPUT).initial_register(Esp, 0x4ffc)
            .memory(0x24ffc, &pop_source(2)[..4], ReadOnly).stack_fault(0), true, 2),
        expect_restored(Case::preserving_flags("POPAD checks the discarded slot's complete SS span", &[0x61])
            .segmented_only().segment(Segment::Ss, stack_segment(0, 0xffff, false))
            .initial_registers(&INPUT).initial_register(Esp, 0xabcd_fff2)
            .memory(0xfff2, &pop_source(4)[..12], ReadOnly).stack_fault(0), false, 3),
    ];
    for present in [false, true] {
        let case = Case::preserving_flags(format!("PUSHAD checks each slot before a later SS failure, first present={present}"), &[0x60])
            .segmented_only().initial_registers(&INPUT).initial_register(Esp, 0x9004)
            .segment(Segment::Ss, StoredSegment {
                attributes: SegmentAttributes::from_bits(0x1d), ..stack_segment(0x20000, 0x8fff, true)
            });
        cases.push(if present {
            case.memory(0x29000, &[0xa5; 4], ReadWrite)
                .expect_memory(0x29000, &[0x22, 0x22, 0x11, 0x11]).stack_fault(0)
        } else { case.fault(0x29000, 2) });
    }
    cases
}

#[rustfmt::skip]
fn sequences() -> Vec<Sequence> {
    vec![
        Sequence::preserving_flags("PUSHA and POPA preserve the high halves of intervening full writes")
            .initial_registers(&INPUT).initial_register(Esp, 0x9000).memory(0x8ff0, &[0xa5; 16], ReadWrite)
            .step(Step::preserving_flags(&[0x66, 0x60]).register(Esp, 0x8ff0).expect_memory(0x8ff0, &pushed(0x9000, 2)))
            .step(Step::preserving_flags(&[0xb8, 0x10, 0x32, 0x54, 0x76]).register(Eax, 0x7654_3210))
            .step(Step::preserving_flags(&[0x66, 0x61]).register(Esp, 0x9000).register(Eax, 0x7654_2222))
            .step(Step::preserving_flags(&[0x89, 0xc1]).register(Ecx, 0x7654_2222))
            .step(Step::preserving_flags(&[0x89, 0xe2]).register(Edx, 0x9000))
            .step(Step::preserving_flags(&[0x8b, 0x1d, 0, 0x60, 0, 0]).fault(0x6000, 0)),
        Sequence::from_opaque_flags("PUSHA fault publishes earlier arithmetic and keeps its completed stores")
            .initial_registers(&INPUT).initial_register(Esp, 0x9000).memory(0x5000, &[0xa5; 4], ReadWrite)
            .step(Step::preserving_flags(&[0xb8, 0xff, 0xff, 0xff, 0x7f]).register(Eax, 0x7fff_ffff))
            .step(Step::new(&[0x83, 0xc0, 1], Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
                .register(Eax, 0x8000_0000))
            .step(Step::preserving_flags(&[0xbc, 4, 0x50, 0, 0]).register(Esp, 0x5004))
            .step(Step::preserving_flags(&[0x60]).expect_memory(0x5000, &[0, 0, 0, 0x80]).fault(0x4ffc, 2))
            .trailing_code(&[0x89, 0xc7], 1),
        Sequence::from_opaque_flags("POPA fault combines partial restores with prior aliases and arithmetic")
            .initial_registers(&INPUT).initial_register(Esp, 0x9000)
            .memory(0x4ffa, &pop_source(2)[..6], ReadWrite)
            .step(Step::preserving_flags(&[0xbf, 0x10, 0x32, 0x54, 0x76]).register(Edi, 0x7654_3210))
            .step(Step::preserving_flags(&[0x66, 0xbe, 0xef, 0xbe]).register(Esi, 0xbbbb_beef))
            .step(Step::preserving_flags(&[0xb8, 0xff, 0xff, 0xff, 0x7f]).register(Eax, 0x7fff_ffff))
            .step(Step::new(&[0x83, 0xc0, 1], Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
                .register(Eax, 0x8000_0000))
            .step(Step::preserving_flags(&[0xbc, 0xfa, 0x4f, 0, 0]).register(Esp, 0x4ffa))
            .step(Step::preserving_flags(&[0x66, 0x61])
                .register(Edi, 0x7654_0304).register(Esi, 0xbbbb_1314).register(Ebp, 0x9999_2324).fault(0x5000, 0))
            .trailing_code(&[0x89, 0xf8], 1),
    ]
}

test_cases!(register_slots_and_stack_wrap, transfers());
test_cases!(faults_retain_completed_slots, fault_progress());
test_sequences!(register_restore_and_fault_progress, sequences());

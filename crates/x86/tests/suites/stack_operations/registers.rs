use super::{code16, stack_segment};
use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
    RegisterExpectation::Exact,
};
use wasm86_x86::{
    Gpr32::{self, *},
    Segment, StoredSegment,
};

#[path = "registers/faults.rs"]
mod faults;
#[path = "registers/progress.rs"]
mod progress;

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

fn widths() -> Vec<Case> {
    let mut cases = Vec::new();
    for big in [false, true] {
        for word in [false, true] {
            for default_word in [false, true] {
                for base in [0, 0x20000] {
                    let width = if word { 2 } else { 4 };
                    let size = (8 * width) as u32;
                    let offset = if big { 0x1234_9000 } else { 0x9000 };
                    let mut prefix = if base == 0 { vec![] } else { vec![0x64, 0x67] };
                    if word != default_word {
                        prefix.push(0x66);
                    }
                    for opcode in [0x60, 0x61] {
                        let code = [prefix.as_slice(), &[opcode]].concat();
                        let mut case = Case::preserving_flags(
                            format!("all registers B={big} word={word} code16={default_word} base={base:x} {code:02x?}"), &code,
                        )
                        .initial_registers(&INPUT)
                        .segment(Segment::Ss, stack_segment(base, u32::MAX, big))
                        .segment(Segment::Fs, StoredSegment::unusable(0));
                        if opcode == 0x60 {
                            case = case
                                .register(Esp, 0x1234_9000, 0x1234_9000 - size)
                                .memory(base + offset - size, &vec![0xa5; 8 * width], ReadWrite)
                                .expect_memory(base + offset - size, &pushed(0x1234_9000, width));
                        } else {
                            case = expect_restored(case, word, 8)
                                .register(Esp, 0x1234_9000, 0x1234_9000 + size)
                                .memory(base + offset, &pop_source(width), ReadOnly);
                        }
                        if default_word {
                            case = case.segment(Segment::Cs, code16());
                        }
                        if default_word || !big || base != 0 {
                            case = case.segmented_only();
                        }
                        cases.push(case);
                    }
                }
            }
        }
    }
    cases
}

fn small_stack_wrap() -> Vec<Case> {
    let mut cases = Vec::new();
    for (width, prefix, push_end, pop_end) in [
        (2, &[0x66][..], 0xabcd_fff8, 0xabcd_0008),
        (4, &[][..], 0xabcd_ffe8, 0xabcd_0018),
    ] {
        let frame = pushed(0xabcd_0008, width);
        let source = pop_source(width);
        for limit in [0xffff, 0x2ffff] {
            let upper_length = frame.len() - 8;
            cases.push(
                Case::preserving_flags(
                    format!("PUSHA wraps between slots, width={width} limit={limit:x}"),
                    &[prefix, &[0x60]].concat(),
                )
                .segmented_only()
                .initial_registers(&INPUT)
                .segment(Segment::Ss, stack_segment(0, limit, false))
                .register(Esp, 0xabcd_0008, push_end)
                .memory(push_end & 0xffff, &vec![0xa5; upper_length], ReadWrite)
                .memory(0, &[0xa5; 8], ReadWrite)
                .expect_memory(push_end & 0xffff, &frame[..upper_length])
                .expect_memory(0, &frame[upper_length..]),
            );
            cases.push(expect_restored(
                Case::preserving_flags(
                    format!("POPA wraps between slots, width={width} limit={limit:x}"),
                    &[prefix, &[0x61]].concat(),
                )
                .segmented_only()
                .initial_registers(&INPUT)
                .segment(Segment::Ss, stack_segment(0, limit, false))
                .register(Esp, 0xabcd_fff8, pop_end)
                .memory(0xfff8, &source[..8], ReadOnly)
                .memory(0, &source[8..], ReadOnly)
                .memory(0x10000, &[0x5a; 32], ReadOnly),
                width == 2,
                8,
            ));
        }
    }
    for (width, prefix, start, end) in [
        (2, &[0x66][..], 0xffff, 0xabcd_000f),
        (4, &[][..], 0xfffe, 0xabcd_001e),
    ] {
        let source = pop_source(width);
        cases.push(expect_restored(
            Case::preserving_flags(
                format!("POPA keeps a straddling slot consecutive then wraps, width={width}"),
                &[prefix, &[0x61]].concat(),
            )
            .segmented_only()
            .initial_registers(&INPUT)
            .segment(Segment::Ss, stack_segment(0, 0x2ffff, false))
            .register(Esp, 0xabcd_0000 | start, end)
            .memory(start, &source[..width], ReadOnly)
            .memory((start + width as u32) & 0xffff, &source[width..], ReadOnly),
            width == 2,
            8,
        ));
    }
    for (width, prefix, start, end) in [
        (2, &[0x66][..], 0xabcd_0001, 0xabcd_fff1),
        (4, &[][..], 0xabcd_0002, 0xabcd_ffe2),
    ] {
        let frame = pushed(start, width);
        cases.push(
            Case::preserving_flags(
                format!("PUSHA keeps a straddling slot consecutive after wrapping, width={width}"),
                &[prefix, &[0x60]].concat(),
            )
            .segmented_only()
            .initial_registers(&INPUT)
            .segment(Segment::Ss, stack_segment(0, 0x2ffff, false))
            .register(Esp, start, end)
            .memory(end & 0xffff, &vec![0xa5; frame.len()], ReadWrite)
            .expect_memory(end & 0xffff, &frame),
        );
    }
    let source = pop_source(4);
    cases.push(expect_restored(
        Case::preserving_flags(
            "POPA permits a skipped slot above 64K when it fits SS.limit",
            &[0x61],
        )
        .segmented_only()
        .initial_registers(&INPUT)
        .segment(Segment::Ss, stack_segment(0, 0x2ffff, false))
        .register(Esp, 0xabcd_fff2, 0xabcd_0012)
        .memory(0xfff2, &source[..16], ReadOnly)
        .memory(2, &source[16..], ReadOnly),
        false,
        8,
    ));
    cases
}

fn memory_and_encoding_boundaries() -> Vec<Case> {
    let mut cases = Vec::new();
    for (width, prefix) in [(2, &[0x66][..]), (4, &[][..])] {
        let frame = pushed(0x5001, width);
        let destination = 0x5001 - frame.len() as u32;
        cases.push(
            Case::preserving_flags(
                format!("PUSHA crosses scattered pages within its first slot, width={width}"),
                &[prefix, &[0x60]].concat(),
            )
            .initial_registers(&INPUT)
            .register(Esp, 0x5001, destination)
            .map_page(4, 0x9000, ReadWrite)
            .map_page(5, 0x7000, ReadWrite)
            .memory(destination, &vec![0xa5; frame.len()], ReadWrite)
            .expect_memory(destination, &frame),
        );
        cases.push(expect_restored(
            Case::preserving_flags(
                format!("POPA crosses scattered pages within its first slot, width={width}"),
                &[prefix, &[0x61]].concat(),
            )
            .initial_registers(&INPUT)
            .register(Esp, 0x4fff, 0x4fff + 8 * width as u32)
            .map_page(4, 0x9000, ReadOnly)
            .map_page(5, 0x7000, ReadOnly)
            .memory(0x4fff, &pop_source(width), ReadOnly),
            width == 2,
            8,
        ));
    }
    for opcode in [0x60, 0x61] {
        let code = [vec![0x66; 14], vec![opcode]].concat();
        let mut case = Case::preserving_flags(
            format!("{opcode:02x} ends at byte fifteen and code page end"),
            &code,
        )
        .at(0x1ff1)
        .initial_registers(&INPUT)
        .instruction_count(u32::MAX);
        if opcode == 0x60 {
            case = case
                .register(Esp, 0x9000, 0x8ff0)
                .memory(0x8ff0, &[0xa5; 16], ReadWrite)
                .expect_memory(0x8ff0, &pushed(0x9000, 2));
        } else {
            case = expect_restored(case, true, 8)
                .register(Esp, 0x9000, 0x9010)
                .memory(0x9000, &pop_source(2), ReadOnly);
        }
        cases.push(case);
    }
    let frame = pushed(0x5020, 4);
    cases.push(
        Case::preserving_flags("PUSHAD applies SS base before linear wrap", &[0x60])
            .segmented_only()
            .initial_registers(&INPUT)
            .segment(Segment::Ss, stack_segment(0xffff_aff0, 0xffff, true))
            .register(Esp, 0x5020, 0x5000)
            .memory(0xffff_fff0, &[0xa5; 16], ReadWrite)
            .memory(0, &[0xa5; 16], ReadWrite)
            .expect_memory(0xffff_fff0, &frame[..16])
            .expect_memory(0, &frame[16..]),
    );
    let source = pop_source(4);
    cases.push(expect_restored(
        Case::preserving_flags("POPAD applies SS base before linear wrap", &[0x61])
            .segmented_only()
            .initial_registers(&INPUT)
            .segment(Segment::Ss, stack_segment(0xffff_aff0, 0xffff, true))
            .register(Esp, 0x5000, 0x5020)
            .memory(0xffff_fff0, &source[..16], ReadOnly)
            .memory(0, &source[16..], ReadOnly),
        false,
        8,
    ));
    cases
}

test_cases!(independent_code_operand_and_stack_widths, widths());
test_cases!(small_stack_wraps_between_register_slots, small_stack_wrap());
test_cases!(
    page_linear_and_instruction_boundaries,
    memory_and_encoding_boundaries()
);

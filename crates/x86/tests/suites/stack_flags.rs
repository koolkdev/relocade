//! User-mode flag images, pending arithmetic and partial-width restoration.

use crate::flags::Flag;
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{self, Clear, Preserved, Set},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    encoding::check_length,
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::{
    CpuState, FlagBytes,
    Gpr32::{Eax, Ebx, Ecx, Esi, Esp},
    StoredFlags, StoredStatusSource,
};

#[derive(Clone, Copy)]
struct FlagImage {
    name: &'static str,
    bits: u32,
    status: u8,
    direct: u8,
}

// The dense fixture masks are CF/PF/AF/ZF/SF/OF and TF/DF/NT/AC/ID.
// Architectural image values are literal, independently of those fixture positions.
#[rustfmt::skip]
const IMAGES: [FlagImage; 15] = [
    FlagImage { name: "clear", bits: 0x0000_0202, status: 0, direct: 0 },
    FlagImage { name: "CF", bits: 0x0000_0203, status: 1, direct: 0 },
    FlagImage { name: "PF", bits: 0x0000_0206, status: 2, direct: 0 },
    FlagImage { name: "AF", bits: 0x0000_0212, status: 4, direct: 0 },
    FlagImage { name: "ZF", bits: 0x0000_0242, status: 8, direct: 0 },
    FlagImage { name: "SF", bits: 0x0000_0282, status: 16, direct: 0 },
    FlagImage { name: "OF", bits: 0x0000_0a02, status: 32, direct: 0 },
    FlagImage { name: "TF", bits: 0x0000_0302, status: 0, direct: 1 },
    FlagImage { name: "DF", bits: 0x0000_0602, status: 0, direct: 2 },
    FlagImage { name: "NT", bits: 0x0000_4202, status: 0, direct: 4 },
    FlagImage { name: "AC", bits: 0x0004_0202, status: 0, direct: 8 },
    FlagImage { name: "ID", bits: 0x0020_0202, status: 0, direct: 16 },
    FlagImage { name: "all", bits: 0x0024_4fd7, status: 63, direct: 31 },
    FlagImage { name: "alternating", bits: 0x0020_4393, status: 21, direct: 21 },
    FlagImage { name: "complementary", bits: 0x0004_0e46, status: 42, direct: 10 },
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

fn status_expectations(bits: u8) -> Flags<FlagExpectation> {
    let bit = |mask| if bits & mask != 0 { Set } else { Clear };
    Flags {
        cf: bit(1),
        pf: bit(2),
        af: bit(4),
        zf: bit(8),
        sf: bit(16),
        of: bit(32),
    }
}

fn stored_flags(status: u8, direct: u8) -> StoredFlags {
    StoredFlags {
        status_source: StoredStatusSource {
            kind: 0,
            reserved: [0x5a, 0xc3, 0x96],
            left: 0x1234_5678,
            right: 0x8765_4321,
        },
        bytes: FlagBytes {
            cf: 0x80 | (status & 1),
            pf: 0x5a | ((status >> 1) & 1),
            af: 0xfe | ((status >> 2) & 1),
            zf: 0xc2 | ((status >> 3) & 1),
            sf: 0x3c | ((status >> 4) & 1),
            of: 0x96 | ((status >> 5) & 1),
            tf: 0x80 | (direct & 1),
            df: 0xfe | ((direct >> 1) & 1),
            nt: 0x5a | ((direct >> 2) & 1),
            ac: 0xc2 | ((direct >> 3) & 1),
            id: 0x3c | ((direct >> 4) & 1),
            ..CpuState::filled(0xa5).flags.bytes
        },
    }
}

fn push_case(name: impl Into<String>, code: &[u8], image: FlagImage) -> Case {
    Case::new(
        name,
        code,
        logical_flags(image.status),
        Flags::all(Preserved),
    )
    .stored_flags(stored_flags(image.status, image.direct))
    .preserve_flag_record()
}

fn pop_case(name: impl Into<String>, code: &[u8], image: FlagImage, word: bool) -> Case {
    let mut case = Case::new(
        name,
        code,
        logical_flags(image.status ^ 63),
        status_expectations(image.status),
    )
    .stored_flags(stored_flags(image.status ^ 63, image.direct ^ 31))
    .expect_direct_flag(Flag::TF, image.direct & 1 != 0)
    .expect_direct_flag(Flag::DF, image.direct & 2 != 0)
    .expect_direct_flag(Flag::NT, image.direct & 4 != 0);
    if !word {
        case = case
            .expect_direct_flag(Flag::AC, image.direct & 8 != 0)
            .expect_direct_flag(Flag::ID, image.direct & 16 != 0);
    }
    case
}

#[rustfmt::skip]
fn concrete_images() -> Vec<Case> {
    let mut cases = Vec::new();
    for image in IMAGES {
        cases.push(push_case(format!("PUSHFD architectural image {}", image.name), &[0x9c], image)
            .register(Esp, 0x9004, 0x9000).memory(0x8fff, &[0xa5; 10], ReadWrite)
            .expect_memory(0x9000, &image.bits.to_le_bytes()));
    }
    cases.push(push_case("PUSHF writes only the low image word", &[0x66, 0x9c], IMAGES[12])
        .register(Esp, 0x5000, 0x4ffe).memory(0x4ffe, &[0xa5; 2], ReadWrite)
        .expect_memory(0x4ffe, &[0xd7, 0x4f]));
    for (input, image) in [(0, IMAGES[0]), (u32::MAX, IMAGES[12]), (0xffdb_b22a, IMAGES[0]), (0x0020_4393, IMAGES[13])] {
        cases.push(pop_case(format!("POPFD restores writable bits from {input:08x}"), &[0x9d], image, false)
            .register(Esp, 0x9000, 0x9004).memory(0x9000, &input.to_le_bytes(), ReadOnly));
    }
    for direct in [8, 16] {
        cases.push(pop_case(format!("POPF preserves raw AC/ID pattern {direct:02x}"), &[0x66, 0x9d], IMAGES[12], true)
            .stored_flags(stored_flags(0, direct)).register(Esp, 0x4ffe, 0x5000)
            .memory(0x4ffe, &[0xff, 0xff], ReadOnly));
    }
    cases
}

fn lazy_images() -> Vec<Case> {
    let mut cases = Vec::new();
    // Each literal image includes fixed IF/bit1 plus TF/DF/NT/AC/ID from the fixture.
    for (name, kind, left, right, status, bits, word) in [
        (
            "byte ADD",
            2,
            0xabcd_007f,
            0x1234_0001,
            52,
            0x0024_4f92,
            true,
        ),
        ("dword SUB", 9, 0x8000_0000, 1, 38, 0x0024_4f16, false),
    ] {
        let image = FlagImage {
            name,
            bits,
            status,
            direct: 31,
        };
        let code = if word { &[0x66, 0x9c][..] } else { &[0x9c][..] };
        let width = if word { 2 } else { 4 };
        let record = StoredFlags {
            status_source: StoredStatusSource {
                kind,
                left,
                right,
                ..stored_flags(status, 31).status_source
            },
            ..stored_flags(status ^ 63, 31)
        };
        cases.push(
            push_case(
                format!("PUSH {name} reads the stored recipe without changing it"),
                code,
                image,
            )
            .stored_flags(record)
            .register(Esp, 0x9004, 0x9004 - width)
            .memory(0x9000, &[0xa5; 8], ReadWrite)
            .expect_memory(0x9004 - width, &bits.to_le_bytes()[..width as usize]),
        );
    }
    cases
}

fn pop_replaces_lazy_status() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, kind, left, status, bits, direct, word) in [
        ("byte ADD", 2, 127, 11, 0x0000_0247, 0, false),
        ("word SUB", 5, 0x8000, 25, 0x0024_47c3, 31, true),
    ] {
        let image = FlagImage {
            name,
            bits,
            status,
            direct,
        };
        let record = StoredFlags {
            status_source: StoredStatusSource {
                kind,
                left,
                right: 1,
                ..stored_flags(status, direct).status_source
            },
            ..stored_flags(status, direct ^ 31)
        };
        cases.push(
            pop_case(
                format!("POP flags replaces pending {name}"),
                if word { &[0x66, 0x9d] } else { &[0x9d] },
                image,
                word,
            )
            .stored_flags(record)
            .register(Esp, 0x9000, if word { 0x9002 } else { 0x9004 })
            .memory(0x9000, &bits.to_le_bytes(), ReadOnly),
        );
    }
    cases
}

#[rustfmt::skip]
fn memory_access() -> Vec<Case> {
    vec![
        push_case("PUSHFD writes its complete image across scattered pages", &[0x9c], IMAGES[13])
            .register(Esp, 0x5003, 0x4fff).map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .memory(0x4ffe, &[0xa5; 6], ReadWrite).expect_memory(0x4fff, &[0x93, 0x43, 0x20, 0]),
        pop_case("POPF reads across scattered pages", &[0x66, 0x9d], IMAGES[13], true)
            .register(Esp, 0x4fff, 0x5001).map_page(4, 0x8000, ReadOnly).map_page(5, 0xa000, ReadOnly)
            .memory(0x4fff, &[0x93, 0x43], ReadOnly),
        Case::new("PUSHFD read-only second page leaves the image, ESP and flags untouched", &[0x9c],
            logical_flags(63), Flags::all(Preserved))
            .stored_flags(stored_flags(63, 31)).preserve_flag_record().initial_register(Esp, 0x5003)
            .memory(0x4fff, &[0xa5], ReadWrite).memory(0x5000, &[0x5a; 3], ReadOnly).fault(0x5000, 3),
    ]
}

fn popped(code: &[u8], status: u8, direct: u8, word: bool) -> Step {
    let mut step = Step::new(code, status_expectations(status))
        .expect_direct_flag(Flag::TF, direct & 1 != 0)
        .expect_direct_flag(Flag::DF, direct & 2 != 0)
        .expect_direct_flag(Flag::NT, direct & 4 != 0);
    if !word {
        step = step
            .expect_direct_flag(Flag::AC, direct & 8 != 0)
            .expect_direct_flag(Flag::ID, direct & 16 != 0);
    }
    step
}

fn histories() -> Vec<Sequence> {
    let mut cases = vec![
        Sequence::new(
            "PUSHFD observes pending INC with preserved carry",
            Flags::all(false),
        )
        .stored_flags(stored_flags(0, 21))
        .initial_registers(&[(Eax, 0x7fff_ffff), (Esp, 0x9004)])
        .memory(0x9000, &[0xa5; 8], ReadWrite)
        .step(Step::new(
            &[0xf9],
            Flags {
                cf: Set,
                ..Flags::all(Preserved)
            },
        ))
        .step(Step::new(&[0x40], status_expectations(55)).register(Eax, 0x8000_0000))
        .step(
            Step::preserving_flags(&[0x9c])
                .register(Esp, 0x9000)
                .expect_memory(0x9000, &[0x97, 0x4b, 0x20, 0]),
        )
        .step(
            Step::preserving_flags(&[0x5b])
                .register(Ebx, 0x0020_4b97)
                .register(Esp, 0x9004),
        ),
        Sequence::new(
            "POPFD canonical image and stack write survive a later data fault",
            Flags::all(false),
        )
        .stored_flags(stored_flags(0, 0))
        .initial_registers(&[(Esp, 0x9000), (Esi, 0x6000)])
        .memory(0x9000, &[0xff; 8], ReadWrite)
        .step(popped(&[0x9d], 63, 31, false).register(Esp, 0x9004))
        .step(
            Step::preserving_flags(&[0x9c])
                .register(Esp, 0x9000)
                .expect_memory(0x9000, &[0xd7, 0x4f, 0x24, 0]),
        )
        .step(Step::preserving_flags(&[0x8b, 0x06]).fault(0x6000, 0))
        .trailing_code(&[0xfc, 0x9d], 2),
        Sequence::new(
            "POPF flags feed LAHF, signed comparison and carry arithmetic",
            Flags::all(false),
        )
        .stored_flags(stored_flags(0, 0))
        .initial_registers(&[(Eax, 0x4433_0011), (Ebx, 0), (Ecx, u32::MAX), (Esp, 0x9000)])
        .memory(0x9000, &[0x83, 0x0e, 0x24, 0], ReadWrite)
        .step(popped(&[0x9d], 49, 26, false).register(Esp, 0x9004))
        .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_8311))
        .step(Step::preserving_flags(&[0x0f, 0x9c, 0xc1]).register(Ecx, 0xffff_ff00))
        .step(Step::new(&[0x83, 0xd3, 0], status_expectations(0)).register(Ebx, 1))
        .step(
            Step::preserving_flags(&[0x9c])
                .register(Esp, 0x9000)
                .expect_memory(0x9000, &[2, 6, 0x24, 0]),
        ),
    ];
    for (name, upper, direct, pushed) in [
        ("set", 0x24, 24, [0xd7, 0x4f, 0x24, 0]),
        ("clear", 0, 0, [0xd7, 0x4f, 0, 0]),
    ] {
        cases.push(
            Sequence::new(
                format!("word POPF retains {name} AC/ID from an earlier POPFD"),
                Flags::all(true),
            )
            .stored_flags(stored_flags(63, 31))
            .initial_register(Esp, 0x9000)
            .memory(0x9000, &[2, 2, upper, 0, 0xff, 0xff, 0xa5], ReadWrite)
            .step(popped(&[0x9d], 0, direct, false).register(Esp, 0x9004))
            .step(popped(&[0x66, 0x9d], 63, 7, true).register(Esp, 0x9006))
            .step(
                Step::preserving_flags(&[0x9c])
                    .register(Esp, 0x9002)
                    .expect_memory(0x9002, &pushed),
            ),
        );
    }
    for (code, push, width) in [(&[0x9d][..], false, 4), (&[0x66, 0x9c][..], true, 2)] {
        cases.push(
            Sequence::new(
                format!("failed {code:02x?} preserves completed arithmetic and raw direct flags"),
                logical_flags(0),
            )
            .stored_flags(stored_flags(0, 31))
            .initial_registers(&[
                (Eax, 0x4433_227f),
                (Esp, if push { 0x4fff + width } else { 0x4fff }),
            ])
            .map_page(4, 0x8000, if push { ReadWrite } else { ReadOnly })
            .backing(0x8fff, &[0xff])
            .step(Step::new(&[0x04, 1], status_expectations(52)).register(Eax, 0x4433_2280))
            .step(Step::preserving_flags(code).fault(0x5000, if push { 2 } else { 0 }))
            .trailing_code(&[0xfc, 0x9c], 2),
        );
    }
    cases
}

#[test]
fn complete_stack_flag_forms() {
    for code in [&[0x9c][..], &[0x66, 0x9c], &[0x9d], &[0x66, 0x9d]] {
        check_length(code);
    }
}

test_cases!(canonical_images_and_pop_write_sets, concrete_images());
test_cases!(stored_arithmetic_is_read_without_publication, lazy_images());
test_cases!(pop_replaces_pending_arithmetic, pop_replaces_lazy_status());
test_cases!(complete_memory_access, memory_access());
test_sequences!(flag_images_compose_with_arithmetic_and_faults, histories());

use super::{pop_case, push_case, stored_flags, FlagImage, IMAGES};
use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::{Gpr32::Esp, StoredFlags, StoredStatusSource};

fn concrete_images() -> Vec<Case> {
    let mut cases = Vec::new();
    for image in IMAGES {
        for (prefix, width) in [(&[][..], 4), (&[0x66][..], 2)] {
            let push = [prefix, &[0x9c]].concat();
            let pop = [prefix, &[0x9d]].concat();
            cases.push(
                push_case(
                    format!("PUSH flags {}, {width} bytes", image.name),
                    &push,
                    image,
                )
                .register(Esp, 0x9004, 0x9004 - width)
                .memory(0x8fff, &[0xa5; 10], ReadWrite)
                .expect_memory(0x9004 - width, &image.bits.to_le_bytes()[..width as usize]),
            );
            cases.push(
                pop_case(
                    format!("POP flags {}, {width} bytes", image.name),
                    &pop,
                    image,
                    width == 2,
                )
                .register(Esp, 0x9000, 0x9000 + width)
                .memory(
                    0x8fff,
                    &[&[0xa5][..], &image.bits.to_le_bytes(), &[0x5a]].concat(),
                    ReadOnly,
                ),
            );
        }
    }
    for (input, image) in [
        (0, IMAGES[0]),
        (u32::MAX, IMAGES[12]),
        (0xffdb_b22a, IMAGES[0]),
    ] {
        cases.push(
            pop_case(
                format!("POPFD ignores fixed and reserved image bits {input:08x}"),
                &[0x9d],
                image,
                false,
            )
            .register(Esp, 0x9000, 0x9004)
            .memory(0x9000, &input.to_le_bytes(), ReadOnly),
        );
    }
    for direct in [0, 8, 16, 24] {
        cases.push(
            pop_case(
                format!("word POPF preserves raw AC/ID pattern {direct:02x}"),
                &[0x66, 0x9d],
                IMAGES[12],
                true,
            )
            .stored_flags(stored_flags(0, direct))
            .register(Esp, 0x4ffe, 0x5000)
            .memory(0x4ffe, &[0xff, 0xff], ReadOnly),
        );
    }
    cases
}

fn lazy_images() -> Vec<Case> {
    let mut cases = Vec::new();
    // Each literal image includes fixed IF/bit1 plus TF/DF/NT/AC/ID from the fixture.
    for (name, kind, left, right, status, bits, word) in [
        (
            "byte SUB",
            1,
            0xabcd_0000,
            0x1234_0001,
            23,
            0x0024_4797,
            false,
        ),
        (
            "byte ADD",
            2,
            0xabcd_007f,
            0x1234_0001,
            52,
            0x0024_4f92,
            true,
        ),
        (
            "byte logical",
            3,
            0xabcd_0080,
            0xdead_beef,
            16,
            0x0024_4782,
            false,
        ),
        (
            "word SUB",
            5,
            0xabcd_8000,
            0x1234_0001,
            38,
            0x0024_4f16,
            true,
        ),
        (
            "word ADD",
            6,
            0xabcd_ffff,
            0x1234_0001,
            15,
            0x0024_4757,
            false,
        ),
        (
            "word logical",
            7,
            0xabcd_8000,
            0xdead_beef,
            18,
            0x0024_4786,
            true,
        ),
        ("dword SUB", 9, 0x8000_0000, 1, 38, 0x0024_4f16, false),
        ("dword ADD", 10, 0xffff_ffff, 1, 15, 0x0024_4757, true),
        ("dword logical", 11, 0, 0xdead_beef, 10, 0x0024_4746, false),
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

test_cases!(canonical_images_and_pop_write_sets, concrete_images());
test_cases!(
    stored_arithmetic_sources_are_read_without_publication,
    lazy_images()
);

test_cases!(
    pop_replaces_a_stored_status_recipe,
    pop_replaces_lazy_status()
);

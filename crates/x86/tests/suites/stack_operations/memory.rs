use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::Gpr32::{Ebx, Ecx, Esp};

#[rustfmt::skip]
fn push_address_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, address, bytes) in [
        (
            &[0xff, 0x34, 0x24][..],
            0x5000,
            &[0x55, 0x66, 0x77, 0x88][..],
        ),
        (
            &[0xff, 0x74, 0x24, 0xfe][..],
            0x5000,
            &[0x33, 0x44, 0x55, 0x66],
        ),
        (
            &[0xff, 0x74, 0x8c, 0xf4][..],
            0x5000,
            &[0x55, 0x66, 0x77, 0x88],
        ),
        (&[0x66, 0xff, 0x34, 0x24][..], 0x5002, &[0x55, 0x66]),
        (&[0x66, 0xff, 0x74, 0x24, 0xff][..], 0x5002, &[0x44, 0x55]),
        (&[0x66, 0xff, 0x74, 0x8c, 0xf4][..], 0x5002, &[0x55, 0x66]),
    ] {
        cases.push(Case::preserving_flags(format!("PUSH memory reads old ESP via {code:02x?}"), code)
            .register(Esp, 0x5004, address).initial_register(Ecx, 3)
            .memory(0x5000, &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99], ReadWrite).expect_memory(address, bytes));
    }
    cases
}

#[rustfmt::skip]
fn pop_address_cases() -> Vec<Case> {
    struct Pop { code: &'static [u8], destination: u32, stack: u32, bytes: &'static [u8] }
    [
        Pop { code: &[0x8f, 0x04, 0x24], destination: 0x5004, stack: 0x5004, bytes: &[0x11, 0x22, 0x33, 0x44] },
        Pop { code: &[0x8f, 0x44, 0x24, 0xfc], destination: 0x5000, stack: 0x5004, bytes: &[0x11, 0x22, 0x33, 0x44] },
        Pop { code: &[0x8f, 0x44, 0x24, 0xfe], destination: 0x5002, stack: 0x5004, bytes: &[0x11, 0x22, 0x33, 0x44] },
        Pop { code: &[0x8f, 0x44, 0x8c, 0xf4], destination: 0x5004, stack: 0x5004, bytes: &[0x11, 0x22, 0x33, 0x44] },
        Pop { code: &[0x8f, 0x03], destination: 0x6000, stack: 0x5004, bytes: &[0x11, 0x22, 0x33, 0x44] },
        Pop { code: &[0x8f, 0x04, 0xa5, 0, 0x60, 0, 0], destination: 0x6000, stack: 0x5004, bytes: &[0x11, 0x22, 0x33, 0x44] },
        Pop { code: &[0x66, 0x8f, 0x04, 0x24], destination: 0x5002, stack: 0x5002, bytes: &[0x11, 0x22] },
        Pop { code: &[0x66, 0x8f, 0x44, 0x24, 0xfe], destination: 0x5000, stack: 0x5002, bytes: &[0x11, 0x22] },
        Pop { code: &[0x66, 0x8f, 0x44, 0x8c, 0xf4], destination: 0x5002, stack: 0x5002, bytes: &[0x11, 0x22] },
        Pop { code: &[0x66, 0x8f, 0x03], destination: 0x6000, stack: 0x5002, bytes: &[0x11, 0x22] },
        Pop { code: &[0x66, 0x8f, 0x04, 0xa5, 0, 0x60, 0, 0], destination: 0x6000, stack: 0x5002, bytes: &[0x11, 0x22] },
    ].into_iter().map(|pop| {
        Case::preserving_flags(format!("POP memory uses incremented ESP only in present components: {:02x?}", pop.code), pop.code)
            .register(Esp, 0x5000, pop.stack).initial_register(Ecx, 3).initial_register(Ebx, 0x6000)
            .memory(0x5000, &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99], ReadWrite)
            .memory(0x6000, &[0xa5; 8], ReadWrite).expect_memory(pop.destination, pop.bytes)
    }).collect()
}

#[rustfmt::skip]
fn split_push_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, stack, output) in [(&[0xff, 0x33][..], 0x5003, &[0x78, 0x56, 0x34, 0x12][..]), (&[0x66, 0xff, 0x33][..], 0x5001, &[0x78, 0x56])] {
        for (stack_frame, source_frame) in [(0x9000, 0xc000), (0xa000, 0xe000)] {
            cases.push(Case::preserving_flags(format!("PUSH split source and stack: {code:02x?}, frames {stack_frame:04x}/{source_frame:04x}"), code)
                .register(Esp, stack, 0x4fff).initial_register(Ebx, 0x6fff)
                .map_page(4, 0x8000, ReadWrite).map_page(5, stack_frame, ReadWrite)
                .map_page(6, 0xb000, ReadOnly).map_page(7, source_frame, ReadOnly)
                .memory(0x4ffe, &[0xa5; 6], ReadWrite).memory(0x6ffe, &[0x5a, 0x78, 0x56, 0x34, 0x12, 0x5a], ReadOnly)
                .expect_memory(0x4fff, output));
        }
    }
    cases
}

#[rustfmt::skip]
fn split_pop_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, stack, output) in [(&[0x8f, 0x03][..], 0x5003, &[0x78, 0x56, 0x34, 0x12][..]), (&[0x66, 0x8f, 0x03][..], 0x5001, &[0x78, 0x56])] {
        for (stack_frame, destination_frame) in [(0x9000, 0xc000), (0xa000, 0xe000)] {
            cases.push(Case::preserving_flags(format!("POP split stack and destination: {code:02x?}, frames {stack_frame:04x}/{destination_frame:04x}"), code)
                .register(Esp, 0x4fff, stack).initial_register(Ebx, 0x6fff)
                .map_page(4, 0x8000, ReadOnly).map_page(5, stack_frame, ReadOnly)
                .map_page(6, 0xb000, ReadWrite).map_page(7, destination_frame, ReadWrite)
                .memory(0x4ffe, &[0x5a, 0x78, 0x56, 0x34, 0x12, 0x5a], ReadOnly).memory(0x6ffe, &[0xa5; 6], ReadWrite)
                .expect_memory(0x6fff, output));
        }
    }
    cases
}

test_cases!(push_old_esp_addresses, push_address_cases());
test_cases!(pop_incremented_esp_addresses, pop_address_cases());
test_cases!(split_push_ranges, split_push_cases());
test_cases!(split_pop_ranges, split_pop_cases());

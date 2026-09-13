use super::{pop_case, push_case, stored_flags, IMAGES};
use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::Gpr32::Esp;

fn scattered_pages() -> Vec<Case> {
    let mut cases = Vec::new();
    let image = IMAGES[13];
    for (word, width) in [(false, 4), (true, 2)] {
        for first_bytes in 1..width {
            let address = 0x5000 - first_bytes;
            for push in [false, true] {
                let code = [
                    if word { vec![0x66] } else { vec![] },
                    vec![if push { 0x9c } else { 0x9d }],
                ]
                .concat();
                let permissions = if push { ReadWrite } else { ReadOnly };
                let case = if push {
                    push_case(
                        format!("PUSH {width} bytes split after {first_bytes}"),
                        &code,
                        image,
                    )
                    .register(Esp, address + width, address)
                    .memory(address - 1, &vec![0xa5; width as usize + 2], permissions)
                    .expect_memory(address, &image.bits.to_le_bytes()[..width as usize])
                } else {
                    pop_case(
                        format!("POP {width} bytes split after {first_bytes}"),
                        &code,
                        image,
                        word,
                    )
                    .register(Esp, address, address + width)
                    .memory(
                        address - 1,
                        &[
                            &[0x5a][..],
                            &image.bits.to_le_bytes()[..width as usize],
                            &[0xa5],
                        ]
                        .concat(),
                        permissions,
                    )
                };
                cases.push(
                    case.map_page(4, 0x8000, permissions)
                        .map_page(5, 0xa000, permissions),
                );
            }
        }
    }
    cases
}

fn stack_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for (prefix, width) in [(&[][..], 4), (&[0x66][..], 2)] {
        for (name, first, second, address, error) in [
            ("absent first page", None, None, 0x4fff, 2),
            ("read-only first page", Some(ReadOnly), None, 0x4fff, 3),
            ("absent second page", Some(ReadWrite), None, 0x5000, 2),
            (
                "read-only second page",
                Some(ReadWrite),
                Some(ReadOnly),
                0x5000,
                3,
            ),
        ] {
            let mut case = Case::preserving_flags(
                format!("PUSH flags {width} bytes: {name} is atomic"),
                &[prefix, &[0x9c]].concat(),
            )
            .stored_flags(stored_flags(63, 31))
            .initial_register(Esp, 0x4fff + width)
            .backing(0x8ffe, &[0x5a, 0x11])
            .backing(0xa000, &[0x22, 0x33, 0x44, 0xa5])
            .fault(address, error);
            if let Some(permissions) = first {
                case = case.map_page(4, 0x8000, permissions);
            }
            if let Some(permissions) = second {
                case = case.map_page(5, 0xa000, permissions);
            }
            cases.push(case);
        }
        for (present, address) in [(false, 0x4fff), (true, 0x5000)] {
            let mut case = Case::preserving_flags(
                format!("POP flags {width} bytes: absent source at {address:04x} is atomic"),
                &[prefix, &[0x9d]].concat(),
            )
            .stored_flags(stored_flags(21, 10))
            .initial_register(Esp, 0x4fff)
            .backing(0x8ffe, &[0x5a, 0xff])
            .backing(0xa000, &[0xff, 0xff, 0xff, 0xa5])
            .fault(address, 0);
            if present {
                case = case.map_page(4, 0x8000, ReadOnly);
            }
            cases.push(case);
        }
    }
    cases
}

fn stack_arithmetic_and_wrapped_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    let image = IMAGES[12];
    for (word, width) in [(false, 4), (true, 2)] {
        let push = if word { &[0x66, 0x9c][..] } else { &[0x9c][..] };
        let pop = if word { &[0x66, 0x9d][..] } else { &[0x9d][..] };
        let top = 0_u32.wrapping_sub(width);
        cases.push(
            push_case(
                format!("PUSH flags {width} bytes wraps ESP with a complete operand"),
                push,
                image,
            )
            .register(Esp, 0, top)
            .memory(top, &vec![0xa5; width as usize], ReadWrite)
            .expect_memory(top, &image.bits.to_le_bytes()[..width as usize]),
        );
        cases.push(
            pop_case(
                format!("POP flags {width} bytes wraps ESP after a complete operand"),
                pop,
                image,
                word,
            )
            .register(Esp, top, 0)
            .memory(top, &image.bits.to_le_bytes()[..width as usize], ReadOnly),
        );
        let address = if word { u32::MAX } else { u32::MAX - 1 };
        for is_push in [false, true] {
            cases.push(
                Case::preserving_flags(
                    format!("wrapped flag stack operand reaches an absent page zero: {width} bytes, push {is_push}"),
                    if is_push { push } else { pop },
                )
                .stored_flags(stored_flags(63, 31))
                .initial_register(
                    Esp,
                    if is_push {
                        address.wrapping_add(width)
                    } else {
                        address
                    },
                )
                .map_page(0xfffff, 0x8000, ReadWrite)
                .backing(0x8ffe, &[0x11, 0x22])
                .backing(0xa000, &[0x33, 0x44])
                .fault(0, if is_push { 2 } else { 0 }),
            );
        }
    }
    cases.push(
        push_case(
            "word PUSHF borrows through ESP bit 16",
            &[0x66, 0x9c],
            image,
        )
        .register(Esp, 0x1235_0000, 0x1234_fffe)
        .memory(0x1234_fffe, &[0xa5; 2], ReadWrite)
        .expect_memory(0x1234_fffe, &[0xd7, 0x4f]),
    );
    cases.push(
        pop_case(
            "word POPF carries through ESP bit 16",
            &[0x66, 0x9d],
            image,
            true,
        )
        .register(Esp, 0x1234_fffe, 0x1235_0000)
        .memory(0x1234_fffe, &[0xd7, 0x4f], ReadOnly),
    );
    cases
}

test_cases!(scattered_stack_pages, scattered_pages());
test_cases!(failed_stack_transfers_preserve_all_state, stack_faults());
test_cases!(
    default32_stack_arithmetic_and_wrapped_page_faults,
    stack_arithmetic_and_wrapped_faults()
);

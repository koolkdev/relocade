use super::*;
use crate::support::cases::{test_cases, Permissions::ReadOnly};
use wasm86_x86::Gpr32::{Eax, Ecx, Edi, Esi};

fn termination() -> Vec<Case> {
    let mut cases = Vec::new();
    for operation in COMPARISONS {
        for prefix in [0xf2, 0xf3] {
            for width in [1, 2, 4] {
                for backward in [false, true] {
                    for (right, consumed, status) in if prefix == 0xf3 {
                        [([3, 5, 5], 1, 0), ([5, 5, 3], 3, 0), ([5, 5, 5], 3, 10)]
                    } else {
                        [([5, 3, 3], 1, 10), ([3, 3, 5], 3, 10), ([3, 3, 3], 3, 0)]
                    } {
                        for incoming_zf in [0, 0xff] {
                            let mut stored = record(if backward { 0x81 } else { 0xfe });
                            stored.bytes.zf = incoming_zf;
                            let start = if backward { 2 * width } else { 0 };
                            let next = |base: u32| {
                                if backward {
                                    (base + start).wrapping_sub(consumed * width)
                                } else {
                                    base + consumed * width
                                }
                            };
                            let mut case = Case::replacing_flags(
                                format!("{operation:?} {prefix:02x} width {width} DF {backward} consumes {consumed}, status {status}, entry ZF {incoming_zf}"),
                                &code(operation, prefix, width, false, false), flags(status),
                            ).stored_flags(stored)
                                .register(Ecx, 3, 3 - consumed)
                                .register(Edi, 0x6000 + start, next(0x6000))
                                .memory(0x6000, &bytes(&right, width, backward), ReadOnly)
                                .initial_register(Eax, if width == 4 { 5 } else { 0xaabb_0005 });
                            if operation == Operation::Cmps {
                                case = case.register(Esi, 0x4000 + start, next(0x4000)).memory(
                                    0x4000,
                                    &bytes(&[5; 3], width, backward),
                                    ReadOnly,
                                );
                            } else {
                                case = case.initial_register(Esi, 0x9000);
                            }
                            cases.push(case);
                        }
                    }
                }
            }
        }
    }
    cases
}

fn final_flags() -> Vec<Case> {
    let mut cases = Vec::new();
    for operation in COMPARISONS {
        for prefix in [0xf2, 0xf3] {
            for (width, sign, maximum) in [
                (1, 0x80u32, 0xffu32),
                (2, 0x8000, 0xffff),
                (4, 0x8000_0000, u32::MAX),
            ] {
                for (left, right, status) in [
                    (0x55, 0x55, 10),
                    (0, 1, 23),
                    (sign, 1, if width == 1 { 36 } else { 38 }),
                    (sign - 1, maximum, if width == 1 { 49 } else { 51 }),
                    (0x10, 1, 6),
                    (maximum, 0, 18),
                ] {
                    let mut case = Case::replacing_flags(
                        format!("last {operation:?} {prefix:02x} width {width}: {left:x} minus {right:x}"),
                        &code(operation, prefix, width, false, false), flags(status),
                    ).stored_flags(record(0xfe)).register(Ecx, 1, 0)
                        .register(Edi, 0x6000, 0x6000 + width)
                        .initial_register(Eax, left)
                        .memory(0x6000, &right.to_le_bytes()[..width as usize], ReadOnly);
                    if operation == Operation::Cmps {
                        case = case.register(Esi, 0x4000, 0x4000 + width).memory(
                            0x4000,
                            &left.to_le_bytes()[..width as usize],
                            ReadOnly,
                        );
                    }
                    cases.push(case);
                }
            }
        }
    }
    cases
}

fn zero_count() -> Vec<Case> {
    let mut cases = Vec::new();
    for operation in COMPARISONS {
        for prefix in [0xf2, 0xf3] {
            for width in [1, 2, 4] {
                for default16 in [false, true] {
                    for address16 in [false, true] {
                        for lazy in [false, true] {
                            let mut stored = record(0xff);
                            if lazy {
                                stored.status_source.kind = 9;
                            }
                            let case = Case::preserving_flags(
                                format!("zero {operation:?} {prefix:02x} width {width}, address16 {address16}, default16 {default16}, lazy {lazy}"),
                                &code(operation, prefix, width, address16, default16),
                            ).stored_flags(stored).segmented_only()
                                .initial_register(Ecx, if address16 { 0xaaaa_0000 } else { 0 })
                                .initial_register(Esi, u32::MAX).initial_register(Edi, u32::MAX)
                                .segment(Segment::Ds, StoredSegment::unusable(0))
                                .segment(Segment::Es, StoredSegment::unusable(0));
                            cases.push(profile(case, default16));
                        }
                    }
                }
            }
        }
    }
    cases
}

fn address_sizes() -> Vec<Case> {
    let mut cases = Vec::new();
    for operation in COMPARISONS {
        for prefix in [0xf2, 0xf3] {
            for width in [1, 2, 4] {
                for default16 in [false, true] {
                    for address16 in [false, true] {
                        for backward in [false, true] {
                            let start = if backward {
                                0
                            } else if address16 {
                                0x10000 - width
                            } else {
                                0u32.wrapping_sub(width)
                            };
                            let next = if backward {
                                start.wrapping_sub(width)
                            } else {
                                start.wrapping_add(width)
                            };
                            let final_offset = if backward {
                                start.wrapping_sub(2 * width)
                            } else {
                                start.wrapping_add(2 * width)
                            };
                            let alias = |high, offset| {
                                if address16 {
                                    high | (offset & 0xffff)
                                } else {
                                    offset
                                }
                            };
                            let count = if address16 { 0xaaaa_0002 } else { 2 };
                            let right = if prefix == 0xf3 { 5u32 } else { 3 };
                            let mut case = Case::replacing_flags(
                                format!("{operation:?} {prefix:02x} width {width}, address16 {address16}, default16 {default16}, DF {backward}"),
                                &code(operation, prefix, width, address16, default16), flags(if prefix == 0xf3 { 10 } else { 0 }),
                            ).stored_flags(record(u8::from(backward)))
                                .register(Ecx, count, count - 2)
                                .register(Edi, alias(0xbbbb_0000, start), alias(0xbbbb_0000, final_offset))
                                .initial_register(Eax, 5)
                                .memory(if address16 { start & 0xffff } else { start }, &right.to_le_bytes()[..width as usize], ReadOnly)
                                .memory(if address16 { next & 0xffff } else { next }, &right.to_le_bytes()[..width as usize], ReadOnly);
                            if operation == Operation::Cmps {
                                // A distinct source segment permits different bytes at the same wrapped offset.
                                case = case
                                    .segmented_only()
                                    .segment(
                                        Segment::Ds,
                                        StoredSegment {
                                            base: 0x20000,
                                            ..StoredSegment::flat_data32(0x23)
                                        },
                                    )
                                    .register(
                                        Esi,
                                        alias(0xcccc_0000, start),
                                        alias(0xcccc_0000, final_offset),
                                    )
                                    .memory(
                                        0x20000u32.wrapping_add(if address16 {
                                            start & 0xffff
                                        } else {
                                            start
                                        }),
                                        &5u32.to_le_bytes()[..width as usize],
                                        ReadOnly,
                                    )
                                    .memory(
                                        0x20000u32.wrapping_add(if address16 {
                                            next & 0xffff
                                        } else {
                                            next
                                        }),
                                        &5u32.to_le_bytes()[..width as usize],
                                        ReadOnly,
                                    );
                            }
                            cases.push(profile(case, default16));
                        }
                    }
                }
            }
        }
    }
    cases
}

test_cases!(stopping_count_direction_and_incoming_zf, termination());
test_cases!(final_comparison_defines_all_status_flags, final_flags());
test_cases!(
    zero_count_preserves_flags_and_skips_all_accesses,
    zero_count()
);
test_cases!(
    address_size_controls_count_and_wrapping_indices,
    address_sizes()
);

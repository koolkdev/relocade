//! Range exceptions use precision rounding; masked values use the destination grid.

use super::*;

fn underflow_boundaries(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for format in FORMATS {
        let emin = format.minimum_exponent;
        let minimum = format.minimum_normal();
        for (source, output, flags, tiny) in [
            ((TOP, emin), minimum, 0, false),
            ((TOP, emin - format.fraction_bits as u16), 1, 0, true),
            (
                (0_u64.wrapping_sub(2 * format.unit()), emin - 1),
                minimum - 1,
                0,
                true,
            ),
            (
                (TOP, emin - format.fraction_bits as u16 - 1),
                0,
                0x10 | PE,
                true,
            ),
            (
                (TOP + 1, emin - format.fraction_bits as u16 - 1),
                1,
                0x10 | PE | C1,
                true,
            ),
            (
                (u64::MAX, emin - format.fraction_bits as u16 - 2),
                0,
                0x10 | PE,
                true,
            ),
            // Direct subnormal rounding avoids a second rounding of a p-bit candidate.
            (
                (TOP + format.unit() + 1, emin - 1),
                minimum / 2 + 1,
                0x10 | PE | C1,
                true,
            ),
            (
                (TOP + 3 * format.unit() - 1, emin - 1),
                minimum / 2 + 1,
                0x10 | PE,
                true,
            ),
            // Both store minimum normal; only the first has a tiny unbounded result.
            (
                (0_u64.wrapping_sub(3 * (format.unit() / 4)), emin - 1),
                minimum,
                0x10 | PE | C1,
                true,
            ),
            (
                (0_u64.wrapping_sub(format.unit() / 4), emin - 1),
                minimum,
                PE | C1,
                false,
            ),
            // The destination-grid tie is tiny, but the precision-grid tie
            // rounds to minimum normal before the range test.
            (
                (0_u64.wrapping_sub(format.unit()), emin - 1),
                minimum,
                0x10 | PE | C1,
                true,
            ),
            (
                (0_u64.wrapping_sub(format.unit() / 2), emin - 1),
                minimum,
                PE | C1,
                false,
            ),
        ] {
            check_store(
                &mut checks,
                format,
                false,
                StoreCase::masked(source, output, flags),
            );
            let mut unmasked = StoreCase::masked(source, output, flags);
            unmasked.control = 0x036f;
            if tiny {
                unmasked.output = None;
                unmasked.flags = 0x10 | PENDING;
            }
            check_store(&mut checks, format, true, unmasked);
            if flags & PE != 0 {
                let mut precision = StoreCase::masked(source, output, flags | PENDING);
                precision.control = 0x035f;
                check_store(&mut checks, format, true, precision);
            }
        }
        for negative in [false, true] {
            for rc in 0..4 {
                let rounds_to_normal = rc == 0 || (rc == 1 && negative) || (rc == 2 && !negative);
                let mut case = StoreCase::masked(
                    (
                        0_u64.wrapping_sub(format.unit() / 4),
                        (emin - 1) | if negative { 0x8000 } else { 0 },
                    ),
                    (if negative { format.sign() } else { 0 }) | minimum,
                    PE | C1,
                );
                case.control = 0x036f | (rc << 10);
                if !rounds_to_normal {
                    case.output = None;
                    case.flags = 0x10 | PENDING;
                }
                check_store(&mut checks, format, true, case);
            }
        }
        // True extended subnormals, pseudo-denormals and very small normals all
        // use underflow responses; even an unmasked DM must never generate DE.
        for source in [(1, 0), (TOP - 1, 0), (TOP, 0), (TOP, 1)] {
            for negative in [false, true] {
                for rc in 0..4 {
                    let increment = (rc == 1 && negative) || (rc == 2 && !negative);
                    let mut case = StoreCase::masked(
                        (source.0, source.1 | if negative { 0x8000 } else { 0 }),
                        (if negative { format.sign() } else { 0 }) | u64::from(increment),
                        0x10 | PE | if increment { C1 } else { 0 },
                    );
                    case.control = 0x037d | (rc << 10);
                    check_store(&mut checks, format, true, case);
                }
            }
        }
    }
}

fn overflow_boundaries(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for format in FORMATS {
        for negative in [false, true] {
            let sign = if negative { format.sign() } else { 0 };
            let se = format.maximum_exponent | if negative { 0x8000 } else { 0 };
            check_store(
                &mut checks,
                format,
                false,
                StoreCase::masked(
                    (0_u64.wrapping_sub(format.unit()), se),
                    sign | (format.infinity - 1),
                    0,
                ),
            );
            for rc in 0..4 {
                let infinity = rc == 0 || (rc == 1 && negative) || (rc == 2 && !negative);
                let output = sign
                    | if infinity {
                        format.infinity
                    } else {
                        format.infinity - 1
                    };
                let flags = 8 | PE | if infinity { C1 } else { 0 };
                // An exact power beyond the range overflows even with no discarded bits.
                let source = (TOP, se + 1);
                let mut masked = StoreCase::masked(source, output, flags);
                masked.control |= rc << 10;
                check_store(&mut checks, format, false, masked);
                let mut precision = StoreCase::masked(source, output, flags | PENDING);
                precision.control = 0x035f | (rc << 10);
                check_store(&mut checks, format, true, precision);
                let unmasked = StoreCase {
                    source,
                    control: 0x0357 | (rc << 10),
                    empty: false,
                    output: None,
                    flags: 8 | PENDING,
                };
                check_store(&mut checks, format, true, unmasked);
                // The unbounded rounded value can remain within range even
                // when the original value is above the largest finite value.
                let mut edge =
                    StoreCase::masked((u64::MAX, se), output, if infinity { flags } else { PE });
                edge.control = 0x0377 | (rc << 10);
                if infinity {
                    edge.output = None;
                    edge.flags = 8 | PENDING;
                }
                check_store(&mut checks, format, true, edge);
            }
        }
    }
}

test_frontends!(underflow, underflow_boundaries);
test_frontends!(overflow, overflow_boundaries);

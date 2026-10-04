//! Memory division retains source precision and source exception priority.

use super::*;

fn quotients(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for source in [
        Source::Single(0x4040_0000),
        Source::Double(0x4008_0000_0000_0000),
    ] {
        for (extension, result) in [
            (6, (LEADING, 0xc000)), // -6 / 3 = -2
            (7, (LEADING, 0xbffe)), // 3 / -6 = -0.5
        ] {
            check_case(
                &mut checks,
                "memory division order without popping",
                Case {
                    source,
                    extension,
                    left: (0xc000_0000_0000_0000, 0xc001),
                    control: 0x037f,
                    result: Some(result),
                    flags: 0,
                    empty: false,
                },
            );
        }
    }
    // PC24 applies after exact source expansion. The binary64 tail makes both
    // quotients inexact; only the reverse quotient rounds upward and sets C1.
    for (extension, result) in [
        (6, (0xffff_ff00_0000_0000, 0x3ffe)), // 1 / (1 + 2^-52), toward zero
        (7, (LEADING + (1 << 40), 0x3fff)),   // (1 + 2^-52) / 1, toward +infinity
    ] {
        check_case(
            &mut checks,
            "division rounds the quotient after exact source expansion",
            Case {
                source: Source::Double(0x3ff0_0000_0000_0001),
                extension,
                left: (LEADING, 0x3fff),
                control: if extension == 6 { 0x0c7f } else { 0x087f },
                result: Some(result),
                flags: if extension == 6 { 0x20 } else { 0x220 },
                empty: false,
            },
        );
    }
}

fn source_exceptions(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for source in [
        Source::Single(0x8000_0000),
        Source::Double(0x8000_0000_0000_0000),
    ] {
        check_case(
            &mut checks,
            "reverse division preserves the memory zero sign",
            Case {
                source,
                extension: 7,
                left: (LEADING, 0xbfff),
                control: 0x037f,
                result: Some((0, 0)),
                flags: 0,
                empty: false,
            },
        );
        for control in [0x037f, 0x037b] {
            check_case(
                &mut checks,
                "memory zero divisor sets ZE and retains its sign",
                Case {
                    source,
                    extension: 6,
                    left: (LEADING, 0x3fff),
                    control,
                    result: (control & 4 != 0).then_some((LEADING, 0xffff)),
                    flags: 4,
                    empty: false,
                },
            );
        }
    }
    for (source, expanded) in [
        (Source::Single(1), (LEADING, 0x3f6a)),
        (Source::Double(1), (LEADING, 0x3bcd)),
    ] {
        for control in [0x037f, 0x037d] {
            for (extension, result) in [(6, (LEADING, 0x7ffe - expanded.1)), (7, expanded)] {
                check_case(
                    &mut checks,
                    "expanded normal retains the memory source denormal exception",
                    Case {
                        source,
                        extension,
                        left: (LEADING, 0x3fff),
                        control,
                        result: (control & 2 != 0).then_some(result),
                        flags: 2,
                        empty: false,
                    },
                );
            }
        }
        for control in [0x037d, 0x0379] {
            check_case(
                &mut checks,
                "zero divide suppresses the unmasked memory denormal exception",
                Case {
                    source,
                    extension: 7,
                    left: (0, 0x8000),
                    control,
                    result: (control & 4 != 0).then_some((LEADING, 0xffff)),
                    flags: 4,
                    empty: false,
                },
            );
        }
    }
    for (source, quieted) in [
        (Source::Single(0x7f80_0001), (0xc000_0100_0000_0000, 0x7fff)),
        (
            Source::Double(0x7ff0_0000_0000_0001),
            (0xc000_0000_0000_0800, 0x7fff),
        ),
    ] {
        for control in [0x037b, 0x037a] {
            for extension in [6, 7] {
                check_case(
                    &mut checks,
                    "memory signaling NaN determines the zero partner's response",
                    Case {
                        source,
                        extension,
                        left: (0, 0),
                        control,
                        result: (control & 1 != 0).then_some(quieted),
                        flags: 1,
                        empty: false,
                    },
                );
            }
        }
    }
    for (left, source, result, flags) in [
        ((0, 0), 0, INDEFINITE, 1),
        ((0, 0x3fff), 0x3ff0_0000_0000_0000, INDEFINITE, 1),
        ((LEADING, 0x7fff), 0x7ff0_0000_0000_0000, INDEFINITE, 1),
        ((LEADING, 0x3fff), 0x7ff0_0000_0000_0000, (0, 0), 0),
    ] {
        check_case(
            &mut checks,
            "special operands replace the unused quotient calculation",
            Case {
                source: Source::Double(source),
                extension: 6,
                left,
                control: 0x037f,
                result: Some(result),
                flags,
                empty: false,
            },
        );
    }
}

test_frontends!(results, quotients);
test_frontends!(operands, source_exceptions);

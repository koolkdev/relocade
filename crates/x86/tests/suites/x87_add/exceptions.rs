//! Operand responses and exponent limits use the shared arithmetic commitment.

use super::*;

fn operand_responses(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let one = (LEADING, 0x3fff);
    let infinity = (LEADING, 0x7fff);
    let qnan = (0xc000_0000_0000_0042, 0xffff);
    let snan = (LEADING + 0x123, 0x7fff);
    for (instruction, left, right, result, flags) in [
        ([0xde, 0xc1], infinity, infinity, infinity, 0),
        ([0xde, 0xc1], infinity, (LEADING, 0xffff), INDEFINITE, 1),
        ([0xde, 0xe9], infinity, infinity, INDEFINITE, 1),
        ([0xde, 0xe1], one, infinity, infinity, 0),
        ([0xde, 0xe9], one, infinity, (LEADING, 0xffff), 0),
        ([0xde, 0xe1], infinity, one, (LEADING, 0xffff), 0),
        ([0xde, 0xe9], infinity, (LEADING, 0xffff), infinity, 0),
        ([0xde, 0xe9], one, qnan, qnan, 0),
        ([0xde, 0xe1], qnan, one, qnan, 0),
        ([0xde, 0xe1], qnan, (qnan.0, 0x7fff), qnan, 0),
        ([0xde, 0xc1], snan, one, (0xc000_0000_0000_0123, 0x7fff), 1),
        ([0xde, 0xe9], snan, qnan, qnan, 1),
        ([0xde, 0xc1], (1, 0x3fff), qnan, INDEFINITE, 1),
        ([0xde, 0xc1], (1, 0), qnan, qnan, 0),
        ([0xde, 0xc1], (1, 0), infinity, infinity, 2),
        ([0xde, 0xc1], (1, 0), (0, 0x8000), (1, 0), 2),
        ([0xde, 0xc1], (LEADING, 0), (0, 0), (LEADING, 1), 2),
        ([0xde, 0xe9], (1, 0), (1, 0), (0, 0), 2),
    ] {
        check_arithmetic(
            &mut checks,
            instruction,
            "operand priority and original NaN signs",
            Case {
                left,
                right,
                control: 0x0353,
                result: Some(result),
                flags,
            },
        );
    }
    for (left, right, flag) in [
        (snan, one, 1),
        ((1, 0x3fff), one, 1),
        (infinity, (LEADING, 0xffff), 1),
        ((1, 0), one, 2),
    ] {
        check_arithmetic(
            &mut checks,
            [0xde, 0xc1],
            "unmasked operand exception suppresses write and pop",
            Case {
                left,
                right,
                control: 0x0340,
                result: None,
                flags: flag | PENDING,
            },
        );
    }
    check_arithmetic(
        &mut checks,
        [0xde, 0xc1],
        "unmasked precision commits the rounded sum",
        Case {
            left: one,
            right: (LEADING, 0x3fff - 64),
            control: 0x035f,
            result: Some(one),
            flags: PE | PENDING,
        },
    );
}

fn range_responses(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (instruction, left, right, control, result, flags) in [
        (
            [0xde, 0xc1],
            (u64::MAX, 0x7ffe),
            (u64::MAX, 0x7ffe),
            0x037f,
            (LEADING, 0x7fff),
            8 | PE | C1,
        ),
        (
            [0xde, 0xc1],
            (u64::MAX, 0x7ffe),
            (u64::MAX, 0x7ffe),
            0x0f7f,
            (u64::MAX, 0x7ffe),
            8 | PE,
        ),
        (
            [0xde, 0xc1],
            (u64::MAX, 0x7ffe),
            (u64::MAX, 0x7ffe),
            0x0377,
            (u64::MAX, 0x1fff),
            8 | PENDING,
        ),
        (
            [0xde, 0xe9],
            (LEADING + 1, 1),
            (LEADING, 1),
            0x037f,
            (1, 0),
            0,
        ),
        (
            [0xde, 0xe9],
            (LEADING + 1, 1),
            (LEADING, 1),
            0x036f,
            (LEADING, 0x5fc2),
            0x10 | PENDING,
        ),
        (
            [0xde, 0xc1],
            (LEADING - 1, 0),
            (1, 0),
            0x037f,
            (LEADING, 1),
            2,
        ),
        (
            [0xde, 0xe9],
            (LEADING, 1),
            (1, 0),
            0x037f,
            (LEADING - 1, 0),
            2,
        ),
    ] {
        check_arithmetic(
            &mut checks,
            instruction,
            "range response from the original exact sum",
            Case {
                left,
                right,
                control,
                result: Some(result),
                flags,
            },
        );
    }
    for (pc, unit) in [(0, 1_u64 << 40), (2, 1 << 11)] {
        // Exact cancellation leaves just above half a subnormal output unit.
        // Rounding first to PC and then to that grid would lose the low bit.
        for rc in 0..4 {
            let up = rc == 0 || rc == 2;
            check_arithmetic(
                &mut checks,
                [0xde, 0xe9],
                "direct subnormal rounding after cancellation",
                Case {
                    left: (LEADING + unit / 2 + 1, 1),
                    right: (LEADING, 1),
                    control: 0x007f | (pc << 8) | (rc << 10),
                    result: Some((if up { unit } else { 0 }, 0)),
                    flags: 0x10 | PE | if up { C1 } else { 0 },
                },
            );
        }
    }
}

test_frontends!(operands, operand_responses);
test_frontends!(range, range_responses);

fn stack_and_pending(engine: Engine, frontend: Frontend) {
    use crate::support::x87::{set_control, status};
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for masked in [false, true] {
        let code = [0xde, 0xe9];
        let mut image = stack_image(&code, 7, 0x3ffc);
        write_value(&mut image.cpu, 0, (LEADING, 0x3fff));
        write_value(&mut image.cpu, 7, (LEADING, 0x3fff));
        image.cpu.x87.tag_word |= 0xc000;
        image.cpu.x87.status.precision = 0;
        set_control(
            &mut image.cpu.x87.control,
            if masked { 0x0341 } else { 0x0340 },
        );
        let mut result = complete_x87(image.cpu, 2, 0x06e9);
        result.x87.status = status(if masked { 0x4541 } else { 0xfdc1 });
        if masked {
            write_value(&mut result, 0, INDEFINITE);
        }
        checks.check(
            "empty source suppresses or replaces subtraction before pop",
            &code,
            &image,
            &[dispatch(result)],
        );
    }
    let code = [0xd8, 0xe9];
    let mut image = stack_image(&code, 7, 0);
    image.cpu.x87.status = status(0xfd81);
    checks.check(
        "pending exception precedes reverse subtraction",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );
}

test_frontends!(stack, stack_and_pending);

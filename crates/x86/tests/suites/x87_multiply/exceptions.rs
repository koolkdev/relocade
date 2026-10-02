//! Operand priority, suppressed writes and deferred arithmetic exceptions.

use super::*;

fn operand_classes(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let one = (LEADING, 0x3fff);
    let infinity = (LEADING, 0x7fff);
    let snan = (LEADING | 0x123, 0x7fff);
    let qnan = (0xc000_0000_0000_0042, 0xffff);
    for (left, right, result, flags) in [
        ((0, 0x8000), one, (0, 0x8000), 0),
        ((0, 0x8000), (LEADING, 0xbfff), (0, 0), 0),
        (infinity, (LEADING, 0xbfff), (LEADING, 0xffff), 0),
        (infinity, (LEADING, 0xffff), (LEADING, 0xffff), 0),
        (infinity, (0, 0), INDEFINITE, 1),
        (snan, one, (0xc000_0000_0000_0123, 0x7fff), 1),
        (snan, qnan, qnan, 1),
        (
            qnan,
            (0xc000_0000_0000_0050, 0x7fff),
            (0xc000_0000_0000_0050, 0x7fff),
            0,
        ),
        // Equal payloads keep the destination's sign as a defined local policy.
        (qnan, (qnan.0, 0x7fff), qnan, 0),
        (
            (LEADING | 0x124, 0xffff),
            snan,
            (0xc000_0000_0000_0124, 0xffff),
            1,
        ),
        ((1, 0x3fff), qnan, INDEFINITE, 1),
        ((0, 0x7fff), one, INDEFINITE, 1),
        ((0x4000_0000_0000_0001, 0x7fff), one, INDEFINITE, 1),
        ((1, 0), qnan, qnan, 0), // QNaN handling takes priority over DE.
        ((1, 0), one, (1, 0), 2),
        ((LEADING, 0), one, (LEADING, 1), 2), // pseudo-denormal
        ((0, 0x8000), (1, 0), (0, 0x8000), 2),
        (infinity, (1, 0), infinity, 2),
    ] {
        // Overflow and precision are unmasked, exposing spurious numerical flags.
        check_product(
            &mut checks,
            "operand classes and priority",
            Case {
                left,
                right,
                control: 0x0353,
                result: Some(result),
                flags,
            },
        );
    }
    for (left, right, flags) in [
        (snan, (1, 0), 1),
        ((1, 0x3fff), one, 1),
        ((0, 0), infinity, 1),
        ((1, 0), one, 2),
        ((LEADING, 0), one, 2),
        // If continued, this denormal operand would also produce tiny/inexact.
        ((1, 0), (LEADING + 1, 1), 2),
    ] {
        check_product(
            &mut checks,
            "unmasked operand exception suppresses value and pop",
            Case {
                left,
                right,
                control: 0x0340,
                result: None,
                flags: flags | PENDING,
            },
        );
    }
    check_product(
        &mut checks,
        "unmasked precision still commits and pops",
        Case {
            left: (LEADING + 1, 0x3fff),
            right: (LEADING + 1, 0x3fff),
            control: 0x035f,
            result: Some((LEADING + 2, 0x3fff)),
            flags: PE | PENDING,
        },
    );
}

fn stack_and_pending(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for empty_slot in [0, 7] {
        for masked in [false, true] {
            let code = [0xde, 0xc9, 0x9b];
            let mut image = stack_image(&code, 7, 0x3ffc | (3 << (empty_slot * 2)));
            image.cpu.x87.status.precision = 0;
            set_control(
                &mut image.cpu.x87.control,
                if masked { 0x0341 } else { 0x0340 },
            );
            // Stack underflow overrides both NaN propagation and operand DE.
            write_register_bits(&mut image.cpu, 0, (0xffff_ffff_ffff_ffff, 0x7fff));
            write_register_bits(&mut image.cpu, 7, (1, 0));
            let mut product = complete_x87(image.cpu, 2, 0x06c9);
            product.x87.status = status(if masked { 0x4541 } else { 0xfdc1 });
            if masked {
                write_value(&mut product, 0, INDEFINITE);
                product.x87.tag_word |= 0xc000;
            }
            let mut waited = product;
            let exit = if masked {
                waited.eip += 1;
                waited.instruction_count += 1;
                Exit::Dispatch(waited.eip)
            } else {
                Exit::FloatingPoint
            };
            checks.check(
                "empty operand takes priority",
                &code,
                &image,
                &[
                    dispatch(product),
                    Step {
                        cpu: waited,
                        ram: &[],
                        exit,
                    },
                ],
            );
        }
    }
    for code in [[0xd8, 0xc9], [0xdc, 0xc9], [0xde, 0xc9]] {
        let mut image = stack_image(&code, 7, 0);
        image.cpu.x87.status = status(0xfd81);
        checks.check(
            "pending exception blocks multiplication",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::FloatingPoint,
            }],
        );
    }
}

test_frontends!(operands, operand_classes);
test_frontends!(stack, stack_and_pending);

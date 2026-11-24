//! Binary32 factors have an exact product at both wider x87 precisions.

use super::*;

const CODE: &[u8] = &[
    0xd9, 0x05, 0, 0x40, 0, 0, // FLD m32
    0xd8, 0x0d, 4, 0x40, 0, 0, // FMUL m32
];

#[test]
fn exact_products_and_precision_control() {
    // (1 + 2^-23)^2 = 1 + 2^-22 + 2^-46. PC24 must round it.
    let exact = LEADING | (1 << 41) | (1 << 17);
    let rounded_up = LEADING | (1 << 41) | (1 << 40);
    for (pc, rc, significand, pe, c1) in [
        (2, 0, exact, 0, 0),
        (2, 1, exact, 0, 0),
        (3, 2, exact, 0, 0),
        (3, 3, exact, 0, 0),
        (0, 2, rounded_up, 1, 1),
    ] {
        let mut image = stack_image(CODE, 0, 0xffff);
        image.cpu.x87.control.precision_control = pc;
        image.cpu.x87.control.rounding_control = rc;
        image.cpu.x87.status.precision = 0;
        image.map(4, 0x8000, false);
        let module = TestModule::new(
            &Compiler::new(SegmentProfile::Flat32)
                .specialize_on_cpu(&image.cpu)
                .compile_block(0x1000, CODE, 2)
                .unwrap(),
        );
        if pc >= 2 {
            assert_eq!(
                operators(&module)
                    .filter(|op| matches!(op, Operator::F64Mul))
                    .count(),
                1
            );
            assert!(
                !operators(&module).any(|op| matches!(op, Operator::I64Mul)),
                "an exact product needs no significand multiplication or rounding residual"
            );
        }

        for (left, right, result, pe, c1) in [
            (
                0x3f80_0001_u32,
                0x3f80_0001_u32,
                (significand, 0x3fff),
                pe,
                c1,
            ),
            (0x8000_0000, 0x3f80_0000, (0, 0x8000), 0, 0),
            // The smallest normal binary32 squared is still normal binary64.
            (0x0080_0000, 0x0080_0000, (LEADING, 0x3f03), 0, 0),
        ] {
            image.data(0x8000, &left.to_le_bytes());
            image.data(0x8004, &right.to_le_bytes());
            let mut cpu = complete_x87(image.cpu, 6, 0x0105);
            cpu = complete_x87(cpu, 6, 0x000d);
            cpu.x87.data_offset = 0x4004;
            cpu.x87.data_selector = 0x23;
            cpu.x87.status.top = 7;
            cpu.x87.status.precision = pe;
            cpu.x87.status.c1 = c1;
            write_value(&mut cpu, 7, result);
            assert_eq!(
                module.observe(&image.input(), 1),
                expected(&image, &[dispatch(cpu)])
            );
        }
    }
}

#[test]
fn denormal_memory_factor_restarts_after_the_load() {
    let mut image = stack_image(CODE, 0, 0xffff);
    image.map(4, 0x8000, false);
    image.data(0x8000, &1_f32.to_le_bytes());
    image.data(0x8004, &1_u32.to_le_bytes()); // least positive subnormal
    let module = TestModule::new(
        &Compiler::new(SegmentProfile::Flat32)
            .specialize_on_cpu(&image.cpu)
            .compile_block(0x1000, CODE, 2)
            .unwrap(),
    );
    let mut loaded = complete_x87(image.cpu, 6, 0x0105);
    loaded.x87.data_offset = 0x4000;
    loaded.x87.data_selector = 0x23;
    loaded.x87.status.top = 7;
    loaded.x87.status.c1 = 0;
    write_value(&mut loaded, 7, (LEADING, 0x3fff));
    assert_eq!(
        module.observe(&image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: loaded,
                ram: &[],
                exit: Exit::Interpret,
            }]
        )
    );
}

//! Scaling retains native significands and restarts exactly at exceptional ranges.

use super::*;
use crate::support::wasm::operators;
use crate::support::{execution::CompiledExecution, machine::expected, step::TestModule};
use relocade_x86::{Compiler, CpuState};
use wasmparser::Operator;

fn native_significand() {
    let code = [
        0xd9, 0xe8, // FLD1: scale by one
        0xdd, 0x05, 0, 0x40, 0, 0, // FLD m64
        0xd9, 0xfd, // FSCALE
        0xdc, 0x05, 0, 0x40, 0, 0, // FADD m64
    ];
    let mut observed = CpuState::default();
    set_control(&mut observed.x87.control, 0x027f);
    let compiled = Compiler::new(SegmentProfile::Flat32)
        .specialize_on_cpu(&observed)
        .compile_block(0x1000, &code, 4)
        .unwrap();
    assert!(operators(&compiled.bytes).any(|op| matches!(op, Operator::F64Add)));
    let module = TestModule::new(&compiled);
    let mut image = stack_image(&code, 0, 0xffff);
    image.cpu.x87.control = observed.x87.control;
    image.cpu.x87.status.precision = 0;
    image.map(4, 0x8000, false);
    image.data(0x8000, &1.5_f64.to_le_bytes());
    let mut result = complete_x87(image.cpu, 2, 0x01e8);
    result = complete_x87(result, 6, 0x0505);
    result = complete_x87(result, 2, 0x01fd);
    result = complete_x87(result, 6, 0x0405);
    result.x87.status.top = 6;
    result.x87.status.c1 = 0;
    result.x87.data_offset = 0x4000;
    result.x87.data_selector = 0x23;
    write_value(&mut result, 7, (LEAD, 0x3fff));
    write_value(&mut result, 6, (0x9000_0000_0000_0000, 0x4001)); // 1.5 * 2 + 1.5 = 4.5
    assert_eq!(
        module.observe(&image.input(), 1),
        expected(&image, &[dispatch(result)])
    );
}

fn restart() {
    let compiled = Compiler::new(SegmentProfile::Flat32)
        .compile_block(0x1000, &CODE, 1)
        .unwrap();
    let checks = CompiledExecution::new(&compiled);
    for (left, right, output, flags) in [
        ((LEAD, 1), (LEAD, 0xbfff), (LEAD >> 1, 0), 0),
        ((LEAD, 0x7ffe), (LEAD, 0x3fff), (LEAD, 0x7fff), 0x228),
        ((LEAD, 0x3fff), (LEAD, 0xffff), (0, 0), 0),
    ] {
        let mut image = stack_image(&CODE, 0, 0xffff);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 0, left);
        write_value(&mut image.cpu, 1, right);
        let mut result = complete_x87(image.cpu, 2, 0x01fd);
        result.x87.status.overflow = u8::from(flags & 8 != 0);
        result.x87.status.precision = u8::from(flags & 32 != 0);
        result.x87.status.c1 = u8::from(flags & 512 != 0);
        write_value(&mut result, 0, output);
        checks.check_restart(
            "FSCALE restarts before effects",
            &image,
            &[],
            &dispatch(result),
        );
    }
    // Zero remains compiled even when the scale's finite magnitude is enormous.
    let mut image = stack_image(&CODE, 0, 0xffff);
    write_value(&mut image.cpu, 0, (0, 0x8000));
    write_value(&mut image.cpu, 1, (u64::MAX, 0x7ffe));
    let mut result = complete_x87(image.cpu, 2, 0x01fd);
    result.x87.status.c1 = 0;
    checks.check_native("FSCALE admitted operand", &image, &dispatch(result));
}

#[test]
fn preserves_native_precision_and_checks_range() {
    native_significand();
    restart();
}

#[test]
fn exact_scales_do_not_guard_observed_precision_or_rounding() {
    let module = TestModule::new(
        &Compiler::new(SegmentProfile::Flat32)
            .specialize_on_cpu(&CpuState::default())
            .compile_block(0x1000, &CODE, 1)
            .unwrap(),
    );
    for pc in 0..4 {
        for rc in 0..4 {
            let mut image = stack_image(&CODE, 0, 0xffff);
            set_control(&mut image.cpu.x87.control, 0x007f | (pc << 8) | (rc << 10));
            write_value(&mut image.cpu, 0, (LEAD + 1, 0x3fff));
            write_value(&mut image.cpu, 1, (LEAD, 0x3fff));
            let mut result = complete_x87(image.cpu, 2, 0x01fd);
            result.x87.status.c1 = 0;
            write_value(&mut result, 0, (LEAD + 1, 0x4000));
            assert_eq!(
                module.observe(&image.input(), 1),
                expected(&image, &[dispatch(result)])
            );
        }
    }
}

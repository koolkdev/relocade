//! Precision tracking retains general JIT arithmetic and precise restart boundaries.

use super::*;

fn wider_values_stay_in_jit(engine: Engine) {
    let code = [0xd8, 0xc9];
    let module = TestModule::new(&compiler().compile(0x1000, &code, 1).unwrap());
    for (left, right, product, pe) in [
        // PC53 does not narrow operands already held in extended registers.
        (
            (LEADING + 1, 0x3fff),
            (0xc000_0000_0000_0000, 0x3fff),
            (0xc000_0000_0000_0000, 0x3fff),
            1,
        ),
        // Binary64 rounds this up to its minimum normal; x87 retains it exactly.
        (
            (LEADING, 0x3c01),
            (0xffff_ffff_ffff_f800, 0x3ffe),
            (0xffff_ffff_ffff_f800, 0x3c00),
            0,
        ),
        ((LEADING, 0x3c01), (LEADING, 0x3ffe), (LEADING, 0x3c00), 0),
        // Binary64 underflows to zero or overflows, but these x87 results are normal.
        ((LEADING, 0x3c01), (LEADING, 0x3c01), (LEADING, 0x3803), 0),
        ((LEADING, 0x43fe), (LEADING, 0x4000), (LEADING, 0x43ff), 0),
        // Unknown register values retain the entire extended JIT range.
        ((LEADING, 1), (LEADING, 0x3fff), (LEADING, 1), 0),
        ((LEADING, 0x7ffe), (LEADING, 0x3fff), (LEADING, 0x7ffe), 0),
        ((0, 0x8000), (u64::MAX, 0x7ffe), (0, 0x8000), 0),
    ] {
        let mut image = stack_image(&code, 0, 0xfff0);
        set_control(&mut image.cpu.x87.control, 0x027f);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 0, left);
        write_value(&mut image.cpu, 1, right);
        let mut result = complete_x87(image.cpu, 2, 0x00c9);
        result.x87.status.precision = pe;
        result.x87.status.c1 = 0;
        write_value(&mut result, 0, product);
        assert_eq!(
            engine.observe(&module, &image.input(), 1),
            expected(&image, &[dispatch(result)]),
            "{left:x?} * {right:x?}"
        );
    }
}

fn register_subnormal_range_does_not_raise_denormal(engine: Engine) {
    let code = [0xdb, 0xe2, 0xd8, 0xc8, 0xd8, 0xc8]; // FNCLEX; square twice
    let module = TestModule::new(&compiler().compile(0x1000, &code, 3).unwrap());
    assert_native_multiply(&module);
    let mut image = stack_image(&code, 7, 0x3fff);
    set_control(&mut image.cpu.x87.control, 0x027f);
    // A prior FLD of binary64's smallest subnormal expands it to a normal x87
    // value. Clearing that load's #D must not make register arithmetic raise it again.
    image.cpu.x87.status.denormal = 1;
    write_value(&mut image.cpu, 7, (LEADING, 0x3bcd)); // 2^-1074
    let mut result = image.cpu;
    result.eip += 2;
    result.instruction_count = result.instruction_count.wrapping_add(1);
    result = complete_x87(result, 2, 0x00c8);
    result = complete_x87(result, 2, 0x00c8);
    result.x87.status = status(0x7d00); // TOP=7, existing C0/C2/C3, no exceptions or C1.
    write_value(&mut result, 7, (LEADING, 0x2f37)); // 2^-4296
    assert_eq!(
        engine.observe(&module, &image.input(), 1),
        expected(&image, &[dispatch(result)])
    );
}

fn memory_operand_exceptions_still_restart(engine: Engine) {
    let code = [0xdc, 0x0d, 0, 0x40, 0, 0];
    let compiled = compiler().compile(0x1000, &code, 1).unwrap();
    let module = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    for (bits, product, invalid, denormal) in [
        (1, (LEADING, 0x3bcd), 0, 1),
        (0x7ff0_0000_0000_0001, (0xc000_0000_0000_0800, 0x7fff), 1, 0),
    ] {
        let mut image = stack_image(&code, 0, 0xfffc);
        set_control(&mut image.cpu.x87.control, 0x027f);
        write_value(&mut image.cpu, 0, (LEADING, 0x3fff));
        image.map(4, 0x8000, false);
        image.data(0x8000, &u64::to_le_bytes(bits));
        image.check_unchanged_exit(
            engine,
            &module,
            "raw memory operand keeps its original exception",
            Exit::Interpret,
        );
        let mut result = complete_x87(image.cpu, 6, 0x040d);
        result.x87.status.invalid = invalid;
        result.x87.status.denormal = denormal;
        result.x87.status.c1 = 0;
        result.x87.data_offset = 0x4000;
        result.x87.data_selector = 0x23;
        write_value(&mut result, 0, product);
        assert_eq!(
            engine.observe(&linked, &image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }
}

fn completed_product_survives_later_exits(engine: Engine) {
    for (tail, exit) in [
        (vec![0xd8, 0xca], Exit::Interpret), // infinity operand
        (
            vec![0xa1, 0, 0x50, 0, 0],
            Exit::PageFault {
                address: 0x5000,
                error: 0,
            },
        ),
        (vec![0x9b], Exit::FloatingPoint), // unmasked precision delivered by FWAIT
    ] {
        // The first exact product establishes a PC53 value; the second uses
        // native arithmetic before a later instruction exits.
        let mut code = vec![0xdc, 0x0d, 0, 0x40, 0, 0, 0xdc, 0x0d, 8, 0x40, 0, 0];
        code.extend(tail);
        let module = TestModule::new(&compiler().compile(0x1000, &code, 3).unwrap());
        let mut image = stack_image(&code, 0, 0xffc0);
        set_control(
            &mut image.cpu.x87.control,
            if matches!(exit, Exit::FloatingPoint) {
                0x025f
            } else {
                0x027f
            },
        );
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 0, (LEADING + 0x800, 0x3fff));
        write_value(&mut image.cpu, 2, (LEADING, 0x7fff));
        image.map(4, 0x8000, false);
        image.data(0x8000, &0x3ff0_0000_0000_0000_u64.to_le_bytes());
        image.data(0x8008, &0x3ff8_0000_0000_0000_u64.to_le_bytes());
        let mut result = complete_x87(image.cpu, 6, 0x040d);
        result = complete_x87(result, 6, 0x040d);
        result.x87.data_offset = 0x4008;
        result.x87.data_selector = 0x23;
        result.x87.status.precision = 1;
        result.x87.status.c1 = 1;
        if matches!(exit, Exit::FloatingPoint) {
            result.x87.status.error_summary = 1;
            result.x87.status.busy = 1;
        }
        write_value(&mut result, 0, (0xc000_0000_0000_1000, 0x3fff));
        assert_eq!(
            engine.observe(&module, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: result,
                    ram: &[],
                    exit
                }]
            )
        );
    }
}

#[test]
fn precision53_preserves_extended_values_and_exception_boundaries() {
    wider_values_stay_in_jit(Engine::Wasmtime);
    register_subnormal_range_does_not_raise_denormal(Engine::Wasmtime);
    memory_operand_exceptions_still_restart(Engine::Wasmtime);
    completed_product_survives_later_exits(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_precision53_preserves_extended_values_and_exception_boundaries() {
    wider_values_stay_in_jit(Engine::V8);
    register_subnormal_range_does_not_raise_denormal(Engine::V8);
    memory_operand_exceptions_still_restart(Engine::V8);
    completed_product_survives_later_exits(Engine::V8);
}

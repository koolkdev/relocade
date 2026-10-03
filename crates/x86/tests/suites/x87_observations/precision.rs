//! A masked, already-set precision flag needs no repeated exception bookkeeping.
use super::*;

fn matched_arithmetic(engine: Engine) {
    let code = [0xd8, 0xc9].repeat(8);
    for (pc, rc, significand, c1) in [(3, 0, LEADING + 9, 0), (2, 2, LEADING + (8 << 11), 1)] {
        let block = TestModule::new(
            &masked_precision_compiler(pc, rc)
                .compile(0x1000, &code, 8)
                .unwrap(),
        );
        let mut image = stack_image(&code, 0, 0xfff0);
        image.cpu.x87.control.precision_control = pc;
        image.cpu.x87.control.rounding_control = rc;
        image.cpu.x87.control.precision_mask = 0x21;
        image.cpu.x87.status.precision = 0xa1;
        image.cpu.x87.status.c1 = 0x81;
        write_value(&mut image.cpu, 0, (LEADING + u64::from(pc == 3), 0x3fff));
        write_value(&mut image.cpu, 1, (LEADING + 1, 0x3fff));
        let mut result = image.cpu;
        for _ in 0..8 {
            result = complete_x87(result, 2, 0x00c9);
        }
        result.x87.status.c1 = c1;
        write_value(&mut result, 0, (significand, 0x3fff));
        assert_eq!(
            engine.observe(&block, &image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }
    // Exact division and cancellation keep PE but clear C1.
    for (opcode, bits) in [(0xf1, (LEADING, 0x3fff)), (0xe1, (0, 0))] {
        let code = [0xd8, opcode];
        let block = TestModule::new(
            &masked_precision_compiler(3, 0)
                .compile(0x1000, &code, 1)
                .unwrap(),
        );
        let mut image = stack_image(&code, 0, 0xfff0);
        image.cpu.x87.status.precision = 0x81;
        write_value(&mut image.cpu, 0, (LEADING, 0x4000));
        write_value(&mut image.cpu, 1, (LEADING, 0x4000));
        let mut result = complete_x87(image.cpu, 2, u16::from(opcode));
        result.x87.status.c1 = 0;
        write_value(&mut result, 0, bits);
        assert_eq!(
            engine.observe(&block, &image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }
}

fn mismatch_restarts_before_arithmetic(engine: Engine) {
    let code = [0xb8, 7, 0, 0, 0, 0xd8, 0xc9]; // MOV; FMUL
    let compiled = masked_precision_compiler(3, 0)
        .compile(0x1000, &code, 2)
        .unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    for mask in [1, 0] {
        let mut image = stack_image(&code, 0, 0xfff0);
        image.cpu.x87.control.precision_mask = mask;
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 0, (LEADING + 1, 0x3fff));
        write_value(&mut image.cpu, 1, (LEADING + 1, 0x3fff));
        let mut restart = image.cpu;
        restart.eip += 5;
        restart.instruction_count = restart.instruction_count.wrapping_add(1);
        restart.registers.eax = 7;
        assert_eq!(
            engine.observe(&block, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: restart,
                    ram: &[],
                    exit: Exit::Interpret,
                }]
            )
        );
        let mut result = complete_x87(restart, 2, 0x00c9);
        result.x87.status.precision = 1;
        result.x87.status.c1 = 0;
        write_value(&mut result, 0, (LEADING + 2, 0x3fff));
        if mask == 0 {
            result.x87.status.error_summary = 1;
            result.x87.status.busy = 1;
        }
        assert_eq!(
            engine.observe(&linked, &image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }
}

fn clearing_precision_invalidates_the_guard(engine: Engine) {
    let code = [0xd8, 0xc9, 0xdb, 0xe2, 0xd8, 0xc9]; // FMUL; FNCLEX; FMUL
    let compiled = masked_precision_compiler(3, 0)
        .compile(0x1000, &code, 3)
        .unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    let mut image = stack_image(&code, 0, 0xfff0);
    write_value(&mut image.cpu, 0, (LEADING + 1, 0x3fff));
    write_value(&mut image.cpu, 1, (LEADING + 1, 0x3fff));
    let mut restart = complete_x87(image.cpu, 2, 0x00c9);
    restart.x87.status.c1 = 0;
    write_value(&mut restart, 0, (LEADING + 2, 0x3fff));
    restart.eip += 2;
    restart.instruction_count = restart.instruction_count.wrapping_add(1);
    restart.x87.status.precision = 0;
    assert_eq!(
        engine.observe(&block, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: restart,
                ram: &[],
                exit: Exit::Interpret,
            }]
        )
    );
    let mut result = complete_x87(restart, 2, 0x00c9);
    result.x87.status.precision = 1;
    write_value(&mut result, 0, (LEADING + 3, 0x3fff));
    assert_eq!(
        engine.observe(&linked, &image.input(), 1),
        expected(&image, &[dispatch(result)])
    );
}

fn faults_precede_precision_mismatch(engine: Engine) {
    let code = [0xd8, 0xc9, 0xd9, 0x2d, 0, 0x40, 0, 0, 0xd8, 0xc9];
    let block = TestModule::new(
        &masked_precision_compiler(3, 0)
            .compile(0x1000, &code, 3)
            .unwrap(),
    );
    let mut image = stack_image(&code, 0, 0xfff0);
    write_value(&mut image.cpu, 0, (LEADING, 0x4000));
    write_value(&mut image.cpu, 1, (LEADING, 0x3fff));
    image.map(4, 0x8000, false);
    image.data(0x8000, &0x035f_u16.to_le_bytes()); // Unmask existing PE with FLDCW.
    let mut pending = complete_x87(image.cpu, 2, 0x00c9);
    pending.x87.status.c1 = 0;
    pending.eip += 6;
    pending.instruction_count = pending.instruction_count.wrapping_add(1);
    set_control(&mut pending.x87.control, 0x035f);
    pending.x87.status.error_summary = 1;
    pending.x87.status.busy = 1;
    assert_eq!(
        engine.observe(&block, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: pending,
                ram: &[],
                exit: Exit::FloatingPoint,
            }]
        )
    );

    let code = [0xd8, 0x05, 0, 0x40, 0, 0]; // FADD from an absent page.
    let block = TestModule::new(
        &masked_precision_compiler(3, 0)
            .compile(0x1000, &code, 1)
            .unwrap(),
    );
    let mut image = stack_image(&code, 0, 0xfffc);
    image.cpu.x87.status.precision = 0;
    image.check_unchanged_exit(
        engine,
        &block,
        "source fault before PE mismatch",
        Exit::PageFault {
            address: 0x4000,
            error: 0,
        },
    );
    image.cpu.x87.control.precision_mask = 0;
    image.cpu.x87.status.precision = 1;
    image.cpu.x87.status.error_summary = 1;
    image.cpu.x87.status.busy = 1;
    image.check_unchanged_exit(
        engine,
        &block,
        "pending exception before source and PM checks",
        Exit::FloatingPoint,
    );
}

#[test]
fn masked_precision_observations_preserve_status_and_restart() {
    matched_arithmetic(Engine::Wasmtime);
    mismatch_restarts_before_arithmetic(Engine::Wasmtime);
    clearing_precision_invalidates_the_guard(Engine::Wasmtime);
    faults_precede_precision_mismatch(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_masked_precision_observations_preserve_status_and_restart() {
    matched_arithmetic(Engine::V8);
    mismatch_restarts_before_arithmetic(Engine::V8);
    clearing_precision_invalidates_the_guard(Engine::V8);
    faults_precede_precision_mismatch(Engine::V8);
}

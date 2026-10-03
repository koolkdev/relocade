//! Control writes replace the guarded SSA values and retain restart state.

use super::*;
use crate::support::x87::status;

fn loaded_controls(engine: Engine) {
    let code = [0xd8, 0xc9, 0xd9, 0x2d, 0, 0x40, 0, 0, 0xd8, 0xc9];
    let compiled = compiler(3, 0).compile(0x1000, &code, 3).unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    for word in [0x037f, 0x036f, 0x007f, 0x0b7f] {
        let mut image = stack_image(&code, 0, 0xfff0);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 0, (LEADING, 0x4000));
        write_value(&mut image.cpu, 1, (LEADING, 0x3fff));
        image.map(4, 0x8000, false);
        image.data(0x8000, &u16::to_le_bytes(word));
        let mut restart = complete_x87(image.cpu, 2, 0x00c9);
        restart.x87.status.c1 = 0;
        restart.eip += 6;
        restart.instruction_count = restart.instruction_count.wrapping_add(1);
        set_control(&mut restart.x87.control, word);
        let mut result = complete_x87(restart, 2, 0x00c9);
        result.x87.status.c1 = 0;
        if word & 0x0f00 == 0x0300 {
            assert_eq!(
                engine.observe(&block, &image.input(), 1),
                expected(&image, &[dispatch(result)])
            );
        } else {
            assert_eq!(
                engine.observe(&block, &image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu: restart,
                        ram: &[],
                        exit: Exit::Interpret
                    },]
                )
            );
            assert_eq!(
                engine.observe(&linked, &image.input(), 1),
                expected(&image, &[dispatch(result)])
            );
        }
    }
}

fn initialized_controls(engine: Engine) {
    let code = [0xdb, 0xe3, 0xd9, 0x05, 0, 0x40, 0, 0, 0xd8, 0xc8];
    for (pc, rc, precision_set) in [(3, 0, false), (2, 2, false), (3, 0, true)] {
        let compiler = if precision_set {
            masked_precision_compiler(pc, rc)
        } else {
            compiler(pc, rc)
        };
        let compiled = compiler.compile(0x1000, &code, 3).unwrap();
        let block = TestModule::new(&compiled);
        let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
        let mut image = stack_image(&code, 3, 0);
        set_control(
            &mut image.cpu.x87.control,
            0x007f | (u16::from(pc) << 8) | (u16::from(rc) << 10),
        );
        image.map(4, 0x8000, false);
        image.data(0x8000, &0x4000_0000_u32.to_le_bytes());
        let mut initialized = image.cpu;
        initialized.eip += 2;
        initialized.instruction_count = initialized.instruction_count.wrapping_add(1);
        set_control(&mut initialized.x87.control, 0x037f);
        initialized.x87.status = status(0);
        initialized.x87.tag_word = 0xffff;
        initialized.x87.opcode = 0;
        initialized.x87.instruction_offset = 0;
        initialized.x87.data_offset = 0;
        initialized.x87.instruction_selector = 0;
        initialized.x87.data_selector = 0;
        let mut loaded = complete_x87(initialized, 6, 0x0105);
        loaded.x87.status.top = 7;
        loaded.x87.data_offset = 0x4000;
        loaded.x87.data_selector = 0x23;
        write_value(&mut loaded, 7, (LEADING, 0x4000));
        let mut result = complete_x87(loaded, 2, 0x00c8);
        write_value(&mut result, 7, (LEADING, 0x4001));
        if pc == 3 && rc == 0 && !precision_set {
            assert_eq!(
                engine.observe(&block, &image.input(), 1),
                expected(&image, &[dispatch(result)])
            );
        } else {
            assert_eq!(
                engine.observe(&block, &image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu: loaded,
                        ram: &[],
                        exit: Exit::Interpret
                    },]
                )
            );
            assert_eq!(
                engine.observe(&linked, &image.input(), 1),
                expected(&image, &[dispatch(result)])
            );
        }
    }
}

fn exceptions_precede_mode_mismatch(engine: Engine) {
    let code = [0xd9, 0x1d, 0xfd, 0x4f, 0, 0]; // FSTP m32 crosses into an absent page.
    let compiled = compiler(0, 2).compile(0x1000, &code, 1).unwrap();
    let block = TestModule::new(&compiled);
    let mut image = stack_image(&code, 0, 0xffff);
    image.map(4, 0x8000, true);
    image.check_unchanged_exit(
        engine,
        &block,
        "destination fault before RC mismatch",
        Exit::PageFault {
            address: 0x5000,
            error: 2,
        },
    );
    image.cpu.x87.control.precision_mask = 0;
    image.cpu.x87.status.error_summary = 1;
    image.cpu.x87.status.busy = 1;
    image.check_unchanged_exit(
        engine,
        &block,
        "pending exception before destination and RC checks",
        Exit::FloatingPoint,
    );

    // A PE-clear observation keeps masks dynamic: an unmasked #P commits the
    // result and leaves the next waiting instruction to deliver #MF.
    let code = [0xd8, 0xc9, 0x9b];
    let block = TestModule::new(&compiler(3, 2).compile(0x1000, &code, 2).unwrap());
    let mut image = stack_image(&code, 0, 0xfff0);
    set_control(&mut image.cpu.x87.control, 0x0b5f);
    image.cpu.x87.status.precision = 0;
    write_value(&mut image.cpu, 0, (LEADING + 1, 0x3fff));
    write_value(&mut image.cpu, 1, (LEADING + 1, 0x3fff));
    let mut result = complete_x87(image.cpu, 2, 0x00c9);
    result.x87.status.precision = 1;
    result.x87.status.c1 = 1;
    result.x87.status.error_summary = 1;
    result.x87.status.busy = 1;
    write_value(&mut result, 0, (LEADING + 3, 0x3fff));
    assert_eq!(
        engine.observe(&block, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: result,
                ram: &[],
                exit: Exit::FloatingPoint
            },]
        )
    );
}

#[test]
fn mode_writes_and_exception_boundaries() {
    loaded_controls(Engine::Wasmtime);
    initialized_controls(Engine::Wasmtime);
    exceptions_precede_mode_mismatch(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_mode_writes_and_exception_boundaries() {
    loaded_controls(Engine::V8);
    initialized_controls(Engine::V8);
    exceptions_precede_mode_mismatch(Engine::V8);
}

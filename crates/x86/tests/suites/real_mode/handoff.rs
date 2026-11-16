//! A handoff interpreter supplies the imports required by its own memory model.

use super::*;
use crate::support::{
    machine::{expected, step_returns},
    step::{Event, TestModule},
};
use relocade_compiler::Program;

#[test]
fn handoff_registers_the_real_mode_interpreters_memory_imports() {
    // The caller only imports interpret; its Real16 callee requires physical memory.
    let mut program = Program::new();
    let interpret = crate::abi::INTERPRET.import(&mut program);
    let entry = program
        .function::<crate::abi::ExecutionSignature>(move |body, ()| interpret.tail_call(body, ()));
    program.export("enter", entry).unwrap();
    let module = TestModule::new(&relocade_x86::CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "enter".into(),
        execution_target: Some(crate::Compiler::new(ExecutionProfile::Real16).target()),
    })
    .with_interpreter(TestModule::interpreter_with_profile(
        ExecutionProfile::Real16,
    ));
    let code = [0xa1, 0, 0x20]; // MOV AX,[2000]
    let mut image = image(&code);
    image.data(0x8000, &[0x78, 0x56]);
    let mut input = image.input();
    input.mmio_pages = vec![(2, 0x8000)];
    input.observe_mmio = true;
    let mut cpu = retired(&image, code.len());
    cpu.registers.eax = 0x1111_5678;
    let mut wanted = expected(
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
    wanted.events.insert(
        0,
        Event::MmioRead {
            address: 0x2000,
            bytes: 2,
        },
    );
    assert_eq!(module.observe(&input, 1), step_returns(wanted));
}

#[test]
fn pusha_checks_the_entire_frame_before_its_first_store() {
    let code = [0x60];
    let mut image = image(&code);
    image.cpu.eip = 0x5000;
    image.map(5, 0x3000, false);
    image.cpu.registers.esp = 0xabcd_2008;
    image.map(2, 0x8000, true);
    image.data(0x8000, &[0xee; 8]);
    image.data(0x9ff8, &[0xee; 8]);
    let mut input = image.input();
    // The first four pushes are RAM; the remaining four would access MMIO.
    input.mmio_pages = vec![(1, 0x9000)];
    input.observe_mmio = true;
    let module = TestModule::new(
        &crate::Compiler::new(ExecutionProfile::Real16)
            .compile_block(image.cpu.eip, &code, 1)
            .unwrap(),
    );
    assert_eq!(
        module.observe(&input, 1),
        expected(
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::Interpret,
            }],
        ),
    );
}

#[test]
fn popa_checks_the_entire_frame_before_restoring_registers() {
    let code = [0x61];
    let mut image = image(&code);
    image.cpu.eip = 0x5000;
    image.map(5, 0x3000, false);
    image.cpu.registers.esp = 0xabcd_1ff8;
    image.map(1, 0x9000, true);
    image.data(0x9ff8, &[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0, 0]);
    let mut input = image.input();
    // Later slots need MMIO; none of the earlier RAM values may be restored yet.
    input.mmio_pages = vec![(2, 0x8000)];
    input.observe_mmio = true;
    let module = TestModule::new(
        &crate::Compiler::new(ExecutionProfile::Real16)
            .compile_block(image.cpu.eip, &code, 1)
            .unwrap(),
    );
    assert_eq!(
        module.observe(&input, 1),
        expected(
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::Interpret,
            }],
        ),
    );
}

#[test]
fn interrupt_preflight_checks_the_vector_before_writing_the_stack() {
    for (code, vector) in [([0xcc], 3), ([0xce], 4)] {
        let mut image = super::interrupts::interrupt_image(&code, vector);
        image.cpu.flags.bytes.of = 1;
        image.data(0x8000, &[0xee; 6]);
        let mut input = image.input();
        input.physical_pages.retain(|&(page, _, _)| page != 0);
        input.mmio_pages = vec![(0, 0x7000)];
        input.observe_mmio = true;
        let module = TestModule::new(
            &crate::Compiler::new(ExecutionProfile::Real16)
                .compile_block(image.cpu.eip, &code, 1)
                .unwrap(),
        );
        assert_eq!(
            module.observe(&input, 1),
            expected(
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::Interpret,
                }],
            ),
        );
    }
}

#[test]
fn enter_preflight_preserves_overlapping_display_reads() {
    let code = [0xc8, 0, 0, 2];
    let mut image = image(&code);
    image.cpu.registers.esp = 0xabcd_2010;
    image.cpu.registers.ebp = 0x1234_2010;
    image.map(2, 0x8000, true);
    image.data(0x8000, &[0xee; 16]);
    let module = TestModule::new(
        &crate::Compiler::new(ExecutionProfile::Real16)
            .compile_block(image.cpu.eip, &code, 1)
            .unwrap(),
    );
    let mut cpu = retired(&image, code.len());
    cpu.registers.esp = 0xabcd_200a;
    cpu.registers.ebp = 0x1234_200e;
    assert_eq!(
        module.observe(&image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu,
                ram: &[(0x800a, &[0x0e, 0x20, 0x10, 0x20, 0x10, 0x20])],
                exit: Exit::Dispatch(cpu.eip),
            }]
        ),
    );
}

#[test]
fn enter_level_one_needs_no_display_backing() {
    let code = [0xc8, 0, 0, 1];
    let mut image = image(&code);
    image.cpu.registers.esp = 0xabcd_2010;
    image.cpu.registers.ebp = 0x1234_4000;
    image.map(2, 0x8000, true);
    let module = TestModule::new(
        &crate::Compiler::new(ExecutionProfile::Real16)
            .compile_block(image.cpu.eip, &code, 1)
            .unwrap(),
    );
    let mut cpu = retired(&image, code.len());
    cpu.registers.esp = 0xabcd_200c;
    cpu.registers.ebp = 0x1234_200e;
    assert_eq!(
        module.observe(&image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu,
                ram: &[(0x800c, &[0x0e, 0x20, 0, 0x40])],
                exit: Exit::Dispatch(cpu.eip),
            }]
        ),
    );
}

#[test]
fn enter_checks_display_backing_before_the_first_push() {
    let code = [0xc8, 0, 0, 2];
    let mut image = image(&code);
    image.cpu.registers.esp = 0xabcd_2010;
    image.cpu.registers.ebp = 0x1234_3002;
    image.map(2, 0x8000, true);
    image.data(0x8000, &[0xee; 16]);
    let mut input = image.input();
    input.mmio_pages = vec![(3, 0x9000)];
    input.observe_mmio = true;
    let module = TestModule::new(
        &crate::Compiler::new(ExecutionProfile::Real16)
            .compile_block(image.cpu.eip, &code, 1)
            .unwrap(),
    );
    assert_eq!(
        module.observe(&input, 1),
        expected(
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::Interpret,
            }]
        ),
    );
}

#[test]
fn pusha_interpreter_fallback_observes_remapping_between_pushes() {
    let code = [0x60];
    let mut image = image(&code);
    image.cpu.registers.esp = 0xabcd_3008;
    image.map(2, 0xb000, true);
    image.data(0x9000, &[0xee; 8]);
    image.data(0xa000, &[0xee; 8]);
    image.data(0xbff8, &[0xee; 8]);
    let mut input = image.input();
    input.mmio_pages = vec![(3, 0x9000)];
    input.observe_mmio = true;
    input.mmio_updates = vec![crate::support::step::MmioUpdate {
        // After AX, route the remaining pushes in this page to RAM at A000.
        map: vec![(3 * 8, vec![1, 0, 0, 0, 0, 0xa0, 0, 0])],
        ..Default::default()
    }];
    let interpreter = TestModule::interpreter_with_profile(ExecutionProfile::Real16);
    let module = TestModule::new(
        &crate::Compiler::new(ExecutionProfile::Real16)
            .compile_block(image.cpu.eip, &code, 1)
            .unwrap(),
    )
    .with_interpreter(interpreter);
    let mut cpu = retired(&image, code.len());
    cpu.registers.esp = 0xabcd_2ff8;
    let mut wanted = expected(
        &image,
        &[Step {
            cpu,
            ram: &[
                (0x9006, &[0x11, 0x11]),
                (0xa000, &[0x44, 0x44, 0xef, 0xbe, 0x22, 0x22]),
                (0xbff8, &[0x88, 0x88, 0x77, 0x77, 0x66, 0x66, 8, 0x30]),
            ],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
    wanted.events.insert(
        0,
        Event::MmioWrite {
            address: 0x3006,
            value: vec![0x11, 0x11],
        },
    );
    for module in [interpreter, &module] {
        assert_eq!(module.observe(&input, 1), step_returns(wanted.clone()));
    }
}

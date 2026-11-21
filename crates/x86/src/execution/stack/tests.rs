//! Stack instructions with live mode and segment-cache fields.

use crate::{
    execution::ExecutionModule,
    profile::EntryGuarantees,
    state::{cr0, State},
    support::machine::{check, Exit, Image, Step},
    test_step::TestModule,
    Compiler, CpuState, ExecutionExit, ExecutionExitValues, MemoryBackend, SegmentDefaultSize,
    SegmentProfile, SnapshotInstruction,
};
use relocade_compiler::Program;

fn instruction(bytes: &[u8]) -> TestModule {
    let mut program = Program::new();
    let target = Compiler::new(SegmentProfile::Segmented16)
        .with_memory_backend(MemoryBackend::PhysicalBus)
        .target();
    let module = ExecutionModule::declare(
        &mut program,
        target,
        EntryGuarantees::for_profile(target.profile),
    );
    let decoded = SnapshotInstruction::decode(bytes, 0x1000, SegmentDefaultSize::Bits16).unwrap();
    let function = program.function::<crate::abi::ExecutionSignature>(move |body, ()| {
        let mut execution = module.builder(body, 0x1000)?;
        // Both CPU modes execute this body with live segment caches on the PC bus.
        execution.state = State::new(&module.cpu);
        execution.execute(decoded.bind())?;
        execution.complete(|body, _| body.return_(ExecutionExitValues::step_complete()))
    });
    program.export("step", function).unwrap();
    TestModule::from_bytes(&program.compile().unwrap(), "step", None)
}

#[test]
fn pusha_checks_mode_before_the_first_push() {
    let module = instruction(&[0x60]);
    let mut image = Image::empty();
    image.cpu = CpuState::real_mode();
    image.cpu.eip = 0x1000;
    image.cpu.registers.esp = 0xabcd_0007;
    image.cpu.segments.ss.limit = 0;

    image.check_unchanged_exit(
        &module,
        "real low odd SP",
        Exit::GeneralProtection { error: 0 },
    );
    image.cpu.cr0 |= cr0::PROTECTION_ENABLE;
    image.check_unchanged_exit(
        &module,
        "protected stack limit",
        Exit::StackFault { error: 0 },
    );
}

#[test]
fn far_call_checks_the_stack_span_for_the_live_mode() {
    use crate::{
        support::machine::expected,
        test_step::{Event, SegmentResolution},
        DescriptorTables, Segment, SegmentDescriptor, SegmentDescriptorKind, SegmentLimit,
        SegmentRegister,
    };

    // CALL 0023:00001234 with 32-bit operands reserves eight bytes at SS:FFFA.
    let module = instruction(&[0x66, 0x9a, 0x34, 0x12, 0, 0, 0x23, 0]);
    let mut image = Image::empty();
    image.cpu = CpuState::real_mode();
    image.cpu.eip = 0x1000;
    image.cpu.segments.cs = SegmentRegister::real_mode(Segment::Cs, 0x1b);
    image.cpu.registers.esp = 0xabcd_0002;
    image.map(0xf, 0x8000, true);
    image.data(0x8ffa, &[0xa5; 6]);

    let mut cpu = image.cpu;
    cpu.segments.cs = SegmentRegister::real_mode(Segment::Cs, 0x23);
    cpu.eip = 0x1234;
    cpu.registers.esp = 0xabcd_fffa;
    cpu.instruction_count += 1;
    check(
        &module,
        "real-mode return EIP and CS fit through FFFF",
        &image,
        &[Step {
            cpu,
            ram: &[(0x8ffa, &[0x08, 0x10, 0, 0, 0x1b, 0])],
            exit: Exit::Other(ExecutionExit::StepComplete),
        }],
    );

    image.cpu.cr0 |= cr0::PROTECTION_ENABLE;
    let mut tables = DescriptorTables::default();
    tables.insert(
        0x23,
        SegmentDescriptor::new(
            0,
            SegmentLimit::bytes(0xffff).unwrap(),
            SegmentDescriptorKind::Code {
                readable: true,
                conforming: false,
            },
            SegmentDefaultSize::Bits16,
        ),
    );
    let mut input = image.input();
    input.segment_resolutions = vec![SegmentResolution::new(&tables, Segment::Cs, 0x23)];
    let mut fault = expected(
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::StackFault { error: 0 },
        }],
    );
    fault.events.insert(
        0,
        Event::ResolveSegment {
            segment: Segment::Cs as i32,
            selector: 0x23,
        },
    );
    assert_eq!(module.observe(&input, 1), fault);
}

#[test]
fn enter_checks_the_final_word_after_saving_bp() {
    // After saving BP at 1000h, ENTER checks the word at F000h..=F001h.
    let module = instruction(&[0xc8, 0x00, 0x20, 0]);
    for (protected, limit, exit) in [
        (false, 0xf000, Exit::StackFault { error: 0 }),
        (false, 0xf001, Exit::Other(ExecutionExit::StepComplete)),
        (true, 0xf000, Exit::StackFault { error: 0 }),
        (true, 0xf001, Exit::Other(ExecutionExit::StepComplete)),
    ] {
        let mut image = Image::empty();
        image.cpu = CpuState::real_mode();
        image.cpu.eip = 0x1000;
        image.cpu.registers.esp = 0xabcd_1002;
        image.cpu.registers.ebp = 0x5678_1234;
        image.cpu.segments.ss.limit = limit;
        if protected {
            image.cpu.cr0 |= cr0::PROTECTION_ENABLE;
        }
        image.map(1, 0x3000, true);

        let mut cpu = image.cpu;
        if exit == Exit::Other(ExecutionExit::StepComplete) {
            cpu.registers.esp = 0xabcd_f000;
            cpu.registers.ebp = 0x5678_1000;
            cpu.eip += 4;
            cpu.instruction_count += 1;
        }
        check(
            &module,
            "final ENTER allocation",
            &image,
            &[Step {
                cpu,
                ram: &[(0x3000, &[0x34, 0x12])],
                exit,
            }],
        );
    }
}

#[test]
fn cpl3_stack_flags_and_cli_use_the_same_effective_iopl() {
    let live = [0x9c, 0x9d, 0xfa].map(|opcode| instruction(&[opcode]));
    let hosted = TestModule::interpreter_with_profile(SegmentProfile::Segmented16);
    // The backing IF starts clear. POPF tries to set it and to change IOPL to 1.
    for (modules, stored_iopl, pushed_flags, restored_if, cli_denied) in [
        ([&live[0], &live[1], &live[2]], 0xff, 0x3002u16, 1, false),
        ([&live[0], &live[1], &live[2]], 0xfc, 0x0002u16, 0xfe, true),
        ([hosted; 3], 0xff, 0x0202u16, 0xfe, true),
    ] {
        for (opcode, module) in [0x9c, 0x9d, 0xfa].into_iter().zip(modules) {
            let mut image = Image::new(&[opcode]);
            image.cpu =
                CpuState::protected_mode(SegmentDefaultSize::Bits16, SegmentDefaultSize::Bits16);
            image.cpu.eip = 0x1000;
            image.cpu.registers.esp = 0x2002;
            image.cpu.flags.bytes.if_ = 0xfe;
            image.cpu.flags.bytes.iopl = stored_iopl;
            image.map(2, 0x4000, true);
            image.data(0x4002, &0x1202u16.to_le_bytes());

            if opcode == 0xfa && cli_denied {
                image.check_unchanged_exit(
                    module,
                    "CLI denied at CPL3",
                    Exit::GeneralProtection { error: 0 },
                );
                continue;
            }
            let mut cpu = image.cpu;
            cpu.eip += 1;
            cpu.instruction_count += 1;
            let pushed = pushed_flags.to_le_bytes();
            let ram = match opcode {
                0x9c => {
                    cpu.registers.esp -= 2;
                    &[(0x4000, pushed.as_slice())][..]
                }
                0x9d => {
                    cpu.registers.esp += 2;
                    cpu.flags.bytes.if_ = restored_if;
                    &[]
                }
                0xfa => {
                    cpu.flags.bytes.if_ = 0;
                    &[]
                }
                _ => unreachable!(),
            };
            check(
                module,
                "protected interrupt flags",
                &image,
                &[Step {
                    cpu,
                    ram,
                    exit: Exit::Other(ExecutionExit::StepComplete),
                }],
            );
        }
    }
}

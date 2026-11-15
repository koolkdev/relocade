use crate::{
    execution::ExecutionModule,
    state::State,
    test_step::{returned, Event, Input, Observation, Snapshot, TestModule},
    CompiledModule, Compiler, CpuFeature, CpuModel, CpuState, Exception, ExecutionExit,
    ExecutionExitValues, SegmentDefaultSize, SegmentProfile,
};
use relocade_compiler::Program;

fn timestamp_module(tsc_supported: bool) -> CompiledModule {
    let mut program = Program::new();
    let target = Compiler::new(SegmentProfile::Flat32)
        .with_cpu_model(CpuModel::default().with_feature(CpuFeature::Tsc, tsc_supported))
        .target();
    let module = ExecutionModule::declare(&mut program, target);
    let run = program.function::<crate::abi::ExecutionSignature>(move |body, ()| {
        let mut execution = module.builder(body, 0x1000)?;
        // Exercise the declared instruction requirements with live controls.
        // Production profiles currently admit only TSD=0 and protected CPL3.
        execution.state = State::new(&module.cpu);
        let (instruction, _) =
            crate::decode::snapshot(&[0x0f, 0x31], 0x1000, SegmentDefaultSize::Bits32).unwrap();
        execution.execute(instruction.bind())?;
        execution.complete(|body, _| body.return_(ExecutionExitValues::step_complete()))
    });
    program.export("run", run).unwrap();
    CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
        execution_target: None,
    }
}

#[test]
fn timestamp_requirements_precede_counter_results_and_retirement() {
    let supported = TestModule::new(&timestamp_module(true));
    let unsupported = TestModule::new(&timestamp_module(false));
    for (module, cr4, cs, fault) in [
        // TSD denies CPL3 access before any result is written.
        (
            &supported,
            0x604,
            0x1b,
            Some(Exception::GeneralProtection { error_code: 0 }),
        ),
        // Missing TSC support takes precedence over the same privilege failure.
        (&unsupported, 0x604, 0x1b, Some(Exception::InvalidOpcode)),
        // Clearing TSD permits CPL3; CPL0 is permitted even with TSD set.
        (&supported, 0x600, 0x1b, None),
        (&supported, 0x604, 0x18, None),
    ] {
        let mut initial = CpuState {
            cr4,
            eip: 0x1000,
            instruction_count: 0x1122_3344_5566_7788,
            ..CpuState::default()
        };
        initial.segments.cs.selector = cs;
        initial.registers.eax = 0xaaaa_aaaa;
        initial.registers.edx = 0xbbbb_bbbb;
        let mut expected = initial;
        let exit = if let Some(fault) = fault {
            ExecutionExit::Exception(fault)
        } else {
            expected.registers.eax = 0x5566_7788;
            expected.registers.edx = 0x1122_3344;
            expected.eip = 0x1002;
            expected.instruction_count += 1;
            ExecutionExit::StepComplete
        };
        assert_eq!(
            module.observe(&Input::new(&initial.to_bytes()), 1),
            Observation {
                events: vec![Event::Return {
                    outcome: returned(exit),
                    snapshot: Snapshot {
                        cpu: expected.to_bytes().to_vec(),
                        guest: None
                    },
                }],
                guest_unchanged: true,
                machine_unchanged: true,
            }
        );
    }
}

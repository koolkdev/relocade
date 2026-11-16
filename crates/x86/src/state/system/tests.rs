use crate::{
    state::{Cpu, SegmentReads, State, StateAssumption},
    test_step::{Argument, Event, Input, Observation, Outcome, Snapshot, TestModule},
    CompiledModule, CpuState, ExecutionProfile, SegmentProfile,
};
use relocade_compiler::{Program, I32};
use std::{mem::offset_of, sync::Arc};

#[test]
fn partial_entry_assumptions_leave_other_fields_live() {
    let assumptions: Arc<[StateAssumption]> = [
        StateAssumption::bits(
            offset_of!(CpuState, cr0),
            |cpu: &CpuState| &cpu.cr0,
            crate::state::cr0::PROTECTION_ENABLE,
            crate::state::cr0::PROTECTION_ENABLE,
        ),
        StateAssumption::field(
            offset_of!(CpuState, segments.cs.big),
            |cpu: &CpuState| &cpu.segments.cs.big,
            0,
        ),
        StateAssumption::field(
            offset_of!(CpuState, segments.ds.base),
            |cpu: &CpuState| &cpu.segments.ds.base,
            0x4000,
        ),
    ]
    .into();
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let observe = program.function::<fn() -> [u32; 4]>(move |mut body, ()| {
        let mut state = State::new(&cpu).with_assumptions(assumptions.clone());
        let mut segments = SegmentReads::new(
            &cpu,
            SegmentProfile::Segmented32.into(),
            assumptions.clone(),
        );
        let code = state.code_default_is_32_bit(&mut body, None)?;
        let base = segments.base(&mut body, &crate::Segment::Ds.into())?;
        assert_eq!(body.constant_bits(&code)?, Some(0));
        assert_eq!(body.constant_bits(&base)?, Some(0x4000));
        let cpl = state.privilege_level(&mut body)?;
        let stack = segments.big(&mut body, &crate::Segment::Ss.into())?;
        assert_eq!(body.constant_bits(&cpl)?, None);
        assert_eq!(body.constant_bits(&stack)?, None);
        body.return_([
            code.unsigned().extend::<I32>(),
            cpl.unsigned().extend::<I32>(),
            base,
            stack.unsigned().extend::<I32>(),
        ])
    });
    program.export("observe", observe).unwrap();
    let module = TestModule::from_bytes(&program.compile().unwrap(), "observe", None);
    for (cpl, stack) in [(1, 0), (3, 1)] {
        let mut cpu = CpuState::default();
        cpu.segments.cs.big = 0;
        cpu.segments.cs.selector = cpl;
        cpu.segments.ds.base = 0x4000;
        cpu.segments.ss.big = stack;
        let bytes = cpu.to_bytes();
        let observed = module.observe(&Input::new(&bytes), 1);
        let [Event::Return { outcome, snapshot }] = observed.events.as_slice() else {
            panic!("unexpected events: {:?}", observed.events);
        };
        assert_eq!(
            *outcome,
            Outcome::Returned(
                [0, i32::from(cpl), 0x4000, i32::from(stack)]
                    .into_iter()
                    .map(Argument::I32)
                    .collect()
            )
        );
        assert_eq!(snapshot.cpu, bytes);
        assert!(observed.guest_unchanged && observed.machine_unchanged);
    }
}

#[test]
fn predicates_read_live_control_bits_without_changing_the_snapshot() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let observe = program.function::<fn() -> [u32; 4]>(move |mut body, ()| {
        let mut state = State::new(&cpu);
        let protected = state.protected_mode(&mut body)?.unsigned().extend::<I32>();
        let cpl = state.privilege_level(&mut body)?.unsigned().extend::<I32>();
        let tsd = state
            .timestamp_access_denied(&mut body)?
            .unsigned()
            .extend::<I32>();
        let iopl = state.read_iopl(&mut body)?.unsigned().extend::<I32>();
        body.return_([protected, cpl, tsd, iopl])
    });
    program.export("observe", observe).unwrap();
    let module = TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "observe".into(),
        execution_target: None,
    });
    for (pe, vm, selector, cr4, iopl, expected) in [
        (0, 1, 0xffff, 4, 0xff, [0, 0, 0, 3]),
        (1, 0, 0xfffc, 4, 0xfc, [1, 0, 0, 0]),
        (1, 0, 0xfffd, 4, 0xfd, [1, 1, 1, 1]),
        (1, 0, 0xfffe, 4, 0xfe, [1, 2, 1, 2]),
        (1, 0, 0xffff, 0, 0xff, [1, 3, 0, 3]),
        (1, 0, 0xffff, 4, 0xff, [1, 3, 1, 3]),
        (1, 1, 0xfffc, 4, 0xfe, [0, 3, 1, 2]),
    ] {
        let mut cpu = CpuState::filled(0xa5);
        cpu.cr0 = 0x30 | pe;
        cpu.cr4 = 0x600 | cr4;
        cpu.flags.bytes.vm = 0xfe | vm;
        cpu.flags.bytes.iopl = iopl;
        cpu.segments.cs.selector = selector;
        let bytes = cpu.to_bytes();
        assert_eq!(
            module.observe(&Input::new(&bytes), 1),
            Observation {
                events: vec![Event::Return {
                    outcome: Outcome::Returned(expected.into_iter().map(Argument::I32).collect()),
                    snapshot: Snapshot {
                        cpu: bytes.to_vec(),
                        guest: None
                    },
                }],
                guest_unchanged: true,
                machine_unchanged: true,
            },
            "PE={pe}, VM={vm}, CS={selector:#x}, CR4={cr4:#x}",
        );
    }
}

#[test]
fn admitted_modes_fold_privilege_and_timestamp_predicates() {
    for profile in [
        ExecutionProfile::Real16,
        SegmentProfile::Flat32.into(),
        SegmentProfile::Segmented16.into(),
    ] {
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        let function = program.function::<fn()>(move |mut body, ()| {
            let mut state =
                State::new(&cpu).with_assumptions(crate::profile::state_assumptions(profile));
            let protected = state.protected_mode(&mut body)?;
            let cpl = state.privilege_level(&mut body)?;
            let tsd = state.timestamp_access_denied(&mut body)?;
            let is_protected = profile != ExecutionProfile::Real16;
            assert_eq!(
                body.constant_bits(&protected)?,
                Some(u64::from(is_protected))
            );
            assert_eq!(
                body.constant_bits(&cpl)?,
                Some(if is_protected { 3 } else { 0 })
            );
            assert_eq!(body.constant_bits(&tsd)?, Some(0));
            body.return_(())
        });
        program.export("check", function).unwrap();
        program.compile().unwrap();
    }
}

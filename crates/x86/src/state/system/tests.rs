use crate::support::config::{FLAT32, REAL16, SEGMENTED16};
use crate::{
    state::{Cpu, State},
    test_step::{Argument, Event, Input, Observation, Outcome, Snapshot, TestModule},
    CompiledModule, CpuState,
};
use relocade_compiler::{Program, I32};

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
        config: None,
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
    for config in [REAL16, FLAT32, SEGMENTED16] {
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        let function = program.function::<fn()>(move |mut body, ()| {
            let mut state = State::for_entry(
                &cpu,
                &crate::entry::EntryGuarantees::from_assumptions(config.entry),
            );
            let protected = state.protected_mode(&mut body)?;
            let cpl = state.privilege_level(&mut body)?;
            let tsd = state.timestamp_access_denied(&mut body)?;
            let is_protected = config != REAL16;
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

#[test]
fn segment_and_privilege_reads_share_the_same_selector() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program.function::<fn() -> u32>(move |mut body, ()| {
        let mut state = State::for_entry(
            &cpu,
            &crate::entry::EntryGuarantees::from_assumptions(FLAT32.entry),
        );
        let selector = state
            .segments()
            .read_selector(&mut body, &crate::Segment::Cs.into())?;
        assert_eq!(body.constant_bits(selector.and(3))?, Some(3));
        let cpl = state.privilege_level(&mut body)?;
        assert_eq!(body.constant_bits(&cpl)?, Some(3));
        body.return_(selector.unsigned().extend::<I32>())
    });
    program.export("selector", function).unwrap();
    program.compile().unwrap();
}

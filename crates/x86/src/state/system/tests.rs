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
fn entry_guarantees_fold_control_and_interrupt_flag_reads() {
    use crate::{flags::Flag, profile::EntryGuarantees, EntryAssumptions, PrivilegeLevel};

    for (entry, cpl, interrupt_flag, iopl) in [
        (EntryAssumptions::LIVE, None, None, None),
        (EntryAssumptions::REAL_MODE, Some(0), None, None),
        (EntryAssumptions::FLAT32, Some(3), Some(1), Some(0)),
        (EntryAssumptions::SEGMENTED16, Some(3), Some(1), Some(0)),
        (
            EntryAssumptions {
                interrupt_flag: None,
                iopl: None,
                ..EntryAssumptions::SEGMENTED16
            },
            Some(3),
            None,
            None,
        ),
        (
            EntryAssumptions {
                interrupt_flag: Some(false),
                iopl: Some(PrivilegeLevel::Ring2),
                ..EntryAssumptions::LIVE
            },
            None,
            Some(0),
            Some(2),
        ),
    ] {
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        let function = program.function::<fn()>(move |mut body, ()| {
            let mut state = State::for_entry(&cpu, &EntryGuarantees::for_entry(entry));
            let protected = state.protected_mode(&mut body)?;
            let privilege = state.privilege_level(&mut body)?;
            let tsd = state.timestamp_access_denied(&mut body)?;
            assert_eq!(
                body.constant_bits(&protected)?,
                cpl.map(|level| u64::from(level == 3))
            );
            assert_eq!(body.constant_bits(&privilege)?, cpl);
            assert_eq!(body.constant_bits(&tsd)?, cpl.map(|_| 0));
            let interrupt = state.read_flag(&mut body, Flag::IF)?;
            let io_privilege = state.read_iopl(&mut body)?;
            assert_eq!(body.constant_bits(&interrupt)?, interrupt_flag);
            assert_eq!(body.constant_bits(&io_privilege)?, iopl);
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
            &crate::profile::EntryGuarantees::for_entry(crate::EntryAssumptions::FLAT32),
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

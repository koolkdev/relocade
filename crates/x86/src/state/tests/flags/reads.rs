//! Ordered architectural flag queries share demand for stored status bits.

mod demand;

use super::fixture::initial_cpu;
use crate::alu::{ArithmeticOp, StatusSource};
use crate::flags::{Flag, FlagChange};
use crate::state::{Cpu, State};
use crate::test_step::{
    Argument, Engine, Event, Input, Observation, Outcome, Snapshot, TestModule,
};
use crate::{CompiledModule, CpuState};
use wasm86_compiler::{Program, Signature, Type, I1, I8};

fn assert_reads(
    engine: Engine,
    module: &TestModule,
    initial: &CpuState,
    arguments: &[i32],
    expected: &[i32],
) {
    let input = Input {
        arguments: arguments.iter().copied().map(Argument::I32).collect(),
        ..Input::new(&initial.to_bytes())
    };
    assert_eq!(
        engine.observe(module, &input, 1),
        Observation {
            events: vec![Event::Return {
                outcome: Outcome::Returned(expected.iter().copied().map(Argument::I32).collect()),
                snapshot: Snapshot {
                    cpu: initial.to_bytes().to_vec(),
                    guest: None,
                },
            }],
            guest_unchanged: true,
            machine_unchanged: true,
        },
        "arguments {arguments:?}"
    );
}

fn mixed_history(local_base: bool) -> CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I1; 2],
                results: vec![Type::I1; 14],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                if local_base {
                    state.write_flags(&mut body, ArithmeticOp::Add.apply::<I8>(127, 1).flags)?;
                }
                state.write_flag(&mut body, Flag::ZF, true)?;
                let partial = body.parameter::<I1>(0)?;
                state.write_flags(
                    &mut body,
                    FlagChange::partial([
                        (Flag::CF, false.into()),
                        (Flag::DF, true.into()),
                        (Flag::TF, false.into()),
                        (Flag::ID, true.into()),
                    ])
                    .when(partial),
                )?;
                let replace = body.parameter::<I1>(1)?;
                state.write_flags(
                    &mut body,
                    FlagChange::from(StatusSource::<I8>::Logic { result: 0.into() })
                        .preserving(Flag::AF)
                        .when(replace),
                )?;
                let flags = state.read_flags(
                    &mut body,
                    [
                        Flag::DF,
                        Flag::OF,
                        Flag::AF,
                        Flag::CF,
                        Flag::ZF,
                        Flag::DF,
                        Flag::PF,
                        Flag::SF,
                        Flag::CF,
                        Flag::TF,
                        Flag::NT,
                        Flag::AC,
                        Flag::ID,
                        Flag::TF,
                    ],
                )?;
                body.return_(flags.to_vec())
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

fn check_mixed_history(engine: Engine) {
    let mut initial = initial_cpu();
    initial.flags.bytes.tf = 0x81;
    initial.flags.bytes.df = 0xfe;
    initial.flags.bytes.nt = 0x82;
    initial.flags.bytes.ac = 0x83;
    initial.flags.bytes.id = 0xfe;
    // Stored SUB 7-8 has CF/PF/AF/SF set; local ADD 127+1 has AF/SF/OF set.
    for (local, rows) in [
        (
            false,
            [
                ([0, 0], [0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 1]),
                ([1, 0], [1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 0]),
                ([0, 1], [0, 0, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1]),
                ([1, 1], [1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 0, 1, 1, 0]),
            ],
        ),
        (
            true,
            [
                ([0, 0], [0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1]),
                ([1, 0], [1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1, 0]),
                ([0, 1], [0, 0, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1]),
                ([1, 1], [1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 0, 1, 1, 0]),
            ],
        ),
    ] {
        let module = TestModule::new(&mixed_history(local));
        for (arguments, expected) in rows {
            assert_reads(engine, &module, &initial, &arguments, &expected);
        }
    }
}

#[test]
fn ordered_reads_compose_stored_local_partial_and_conditional_flags_without_publication() {
    check_mixed_history(Engine::Wasmtime);
}

fn cached_reads_and_writes() -> CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I1; 26],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let old_id = state.read_flag(&mut body, Flag::ID)?;
                let mut results = vec![state.read_flag(&mut body, Flag::CF)?, old_id.clone()];
                results.extend(state.read_flags(
                    &mut body,
                    [Flag::ZF, Flag::CF, Flag::DF, Flag::PF, Flag::CF, Flag::AF],
                )?);
                results.push(state.read_flag(&mut body, Flag::AF)?);
                results
                    .extend(state.read_flags(&mut body, [Flag::PF, Flag::AF, Flag::CF, Flag::ZF])?);
                state.write_flags(
                    &mut body,
                    FlagChange::partial([
                        (Flag::CF, false.into()),
                        (Flag::ZF, true.into()),
                        (Flag::DF, false.into()),
                        (Flag::ID, false.into()),
                    ]),
                )?;
                results.extend(state.read_flags(
                    &mut body,
                    [Flag::CF, Flag::ZF, Flag::AF, Flag::DF, Flag::CF],
                )?);
                state.write_flags(&mut body, ArithmeticOp::Add.apply::<I8>(127, 1).flags)?;
                results.extend(state.read_flags(
                    &mut body,
                    [Flag::OF, Flag::CF, Flag::ZF, Flag::PF, Flag::SF, Flag::DF],
                )?);
                results.push(state.read_flag(&mut body, Flag::ID)?);
                results.push(old_id);
                body.return_(results)
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

fn check_cached_reads(engine: Engine) {
    let module = TestModule::new(&cached_reads_and_writes());
    let mut initial = initial_cpu();
    initial.flags.bytes.df = 0x81;
    initial.flags.bytes.id = 0x81;
    assert_reads(
        engine,
        &module,
        &initial,
        &[],
        &[
            1, 1, // Earlier single CF and ID reads.
            0, 1, 1, 1, 1, 1, // ZF, CF, DF, PF, CF, AF from the stored source.
            1, // Single AF read reuses the grouped result.
            1, 1, 1, 0, // Entirely cached PF, AF, CF, ZF.
            0, 1, 1, 0, 0, // Partial CF/ZF/DF writes retain AF.
            1, 0, 0, 0, 1, 0, // New arithmetic source retains the local DF write.
            0, 1, // Current ID is false; its earlier captured value remains true.
        ],
    );
}

#[test]
fn grouped_and_single_reads_share_cache_but_retain_values_across_later_writes() {
    check_cached_reads(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn grouped_flag_queries_in_v8() {
    check_mixed_history(Engine::V8);
    check_cached_reads(Engine::V8);
}

use super::fixture::assert_result;
use crate::alu::ArithmeticOp;
use crate::flags::{Flag, FlagChange, StatusFlag};
use crate::register::Register;
use crate::state::{Cpu, State};
use crate::test_step::TestModule;
use crate::FlagBytes;
use crate::{CompiledModule, CpuState, Gpr32};
use wasm86_compiler::{Program, Signature, Type, I1, I32, I64, I8};

fn initial_cpu() -> CpuState {
    let mut cpu = CpuState::from_bytes(std::array::from_fn(|index| index as u8));
    cpu.flags.status_source.kind = 9;
    cpu.flags.status_source.left = 7;
    cpu.flags.status_source.right = 8;
    cpu.flags.bytes.df = 0xfe;
    cpu.eip = 0x1000;
    cpu.instruction_count = u32::MAX;
    cpu
}

#[test]
fn computed_direction_values_publish_canonical_bytes() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32],
                results: vec![Type::I64],
            },
            |mut body| {
                let value = body.parameter::<I32>(0)?;
                let mut state = State::new(&cpu);
                state.write_flag(&mut body, Flag::DF, value.truncate::<I1>())?;
                state.publish(&mut body, 0x1001, 1)?;
                body.return_(0_u64)
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    let module = TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    });
    let initial = initial_cpu();
    for (input, direction) in [(0, 0), (1, 1), (0x80, 0), (0x81, 1), (-1, 1)] {
        let mut expected = initial;
        expected.flags.bytes.df = direction;
        expected.eip = 0x1001;
        expected.instruction_count = 0;
        assert_result(&module, &initial, &[input], &expected, 0);
    }
}

#[test]
fn direction_publication_keeps_earlier_exits_and_later_definitions_independent() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I1, Type::I32],
                results: vec![Type::I64],
            },
            |mut body| {
                let direction = body.parameter::<I1>(0)?;
                let stop = body.parameter::<I32>(1)?;
                let mut state = State::new(&cpu);
                state.write_flag(&mut body, Flag::DF, &direction)?;
                body.if_(stop.eq(1), |mut arm| {
                    state.publish(&mut arm, 0x1001, 1)?;
                    arm.return_(7_u64)
                })?;
                body.if_(stop.eq(2), |mut arm| {
                    state.publish(&mut arm, 0x1001, 1)?;
                    arm.return_(8_u64)
                })?;
                state.write_flag(&mut body, Flag::DF, direction.xor(true))?;
                state.publish(&mut body, 0x1002, 2)?;
                body.return_(0_u64)
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    let module = TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    });
    let initial = initial_cpu();
    for direction in [0, 1] {
        for stop in [0, 1, 2] {
            let mut expected = initial;
            expected.flags.bytes.df = if stop != 0 { direction } else { direction ^ 1 };
            expected.eip = if stop != 0 { 0x1001 } else { 0x1002 };
            expected.instruction_count = if stop != 0 { 0 } else { 1 };
            assert_result(
                &module,
                &initial,
                &[i32::from(direction), stop],
                &expected,
                match stop {
                    1 => 7,
                    2 => 8,
                    _ => 0,
                },
            );
        }
    }
}

#[test]
fn direction_publication_survives_register_aliases_and_status_changes() {
    for concrete in [false, true] {
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        let function = program
            .function(
                Signature {
                    parameters: vec![Type::I32],
                    results: vec![Type::I64],
                },
                |mut body| {
                    let index = body.parameter::<I32>(0)?;
                    let mut state = State::new(&cpu);
                    state.write_flag(&mut body, Flag::DF, true)?;
                    state
                        .write_flags(&mut body, ArithmeticOp::Subtract.apply::<I32>(4, 5).flags)?;
                    state.write_register(&mut body, Gpr32::Eax, 0x1122_3344)?;
                    let before =
                        state.read_register(&mut body, Register::<I8>::indexed(index.clone()))?;
                    state.write_register(
                        &mut body,
                        Register::<I8>::indexed(index),
                        before.xor(0xff),
                    )?;
                    if concrete {
                        state.write_flags(
                            &mut body,
                            FlagChange::partial([(StatusFlag::CF.into(), false.into())]),
                        )?;
                    }
                    state.publish(&mut body, 0x1004, 4)?;
                    body.return_(before.unsigned().extend::<I64>())
                },
            )
            .unwrap();
        program.export("run", function).unwrap();
        let module = TestModule::new(&CompiledModule {
            bytes: program.compile().unwrap(),
            entry: "run".into(),
        });
        let initial = initial_cpu();
        for (index, parent, value, before) in [
            (0, Gpr32::Eax, 0x1122_33bb, 0x44),
            (4, Gpr32::Eax, 0x1122_cc44, 0x33),
            (7, Gpr32::Ebx, 0x2726_da24, 0x25),
        ] {
            let mut expected = initial;
            expected.flags.bytes.df = 1;
            expected.registers.eax = 0x1122_3344;
            expected.registers[parent] = value;
            if concrete {
                expected.flags.status_source.kind = 0;
                expected.flags.bytes = FlagBytes {
                    cf: 0,
                    pf: 1,
                    af: 1,
                    zf: 0,
                    sf: 1,
                    of: 0,
                    ..expected.flags.bytes
                };
            } else {
                expected.flags.status_source.kind = 9;
                expected.flags.status_source.left = 4;
                expected.flags.status_source.right = 5;
            }
            expected.eip = 0x1004;
            expected.instruction_count = 3;
            assert_result(&module, &initial, &[index], &expected, before);
        }
    }
}

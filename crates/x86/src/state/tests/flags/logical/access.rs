use super::super::fixture::{assert_result, initial_cpu};
use crate::alu::ArithmeticOp;
use crate::flags::{Condition, Flag};
use crate::state::{Cpu, State};
use crate::test_step::TestModule;
use crate::CompiledModule;
use crate::FlagBytes;
use wasm86_compiler::{Program, Signature, Type, I1, I32, I64, I8};

#[test]
fn common_flag_reads_use_low_bits_without_changing_the_record() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I64],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let carry = state.read_flag(&mut body, Flag::CF)?;
                let direction = state.read_flag(&mut body, Flag::DF)?;
                let below = state.condition(&mut body, Condition::B)?;
                let repeated = state.read_flag(&mut body, Flag::DF)?;
                body.return_(
                    carry
                        .unsigned()
                        .extend::<I64>()
                        .or(direction.unsigned().extend::<I64>().shl(1))
                        .or(below.unsigned().extend::<I64>().shl(2))
                        .or(repeated.unsigned().extend::<I64>().shl(3)),
                )
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    let module = TestModule::new(&CompiledModule {
        segment_profile: None,
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    });
    for (kind, left, right, stored_carry, carry) in [
        (0, 0x1234_5678, 0x8765_4321, 0xfe, 0),
        (0, 0x1234_5678, 0x8765_4321, 0xff, 1),
        (9, 0, 1, 0xfe, 1),
        (10, 0x7fff_ffff, 1, 0xff, 0),
        (11, 0x8000_0000, 0xdead_beef, 0xff, 0),
    ] {
        for (stored_direction, direction) in [(0, 0), (1, 1), (0x80, 0), (0x81, 1)] {
            let mut initial = initial_cpu();
            initial.flags.status_source.kind = kind;
            initial.flags.status_source.left = left;
            initial.flags.status_source.right = right;
            initial.flags.bytes.cf = stored_carry;
            initial.flags.bytes.df = stored_direction;
            assert_result(&module, &initial, &[], &initial, carry * 5 + direction * 10);
        }
    }
}

#[test]
fn common_writes_accept_constants_and_computed_bits_and_replace_cached_reads() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32; 2],
                results: vec![Type::I64],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let before_carry = state.read_flag(&mut body, Flag::CF)?;
                let before_direction = state.read_flag(&mut body, Flag::DF)?;
                state.write_flag(&mut body, Flag::CF, false)?;
                state.write_flag(&mut body, Flag::DF, true)?;
                let constant_carry = state.read_flag(&mut body, Flag::CF)?;
                let constant_direction = state.read_flag(&mut body, Flag::DF)?;
                let carry = body.parameter::<I32>(0)?.truncate::<I1>();
                let direction = body.parameter::<I32>(1)?.truncate::<I1>();
                state.write_flag(&mut body, Flag::CF, carry)?;
                state.write_flag(&mut body, Flag::DF, direction)?;
                let after_carry = state.read_flag(&mut body, Flag::CF)?;
                let after_direction = state.read_flag(&mut body, Flag::DF)?;
                state.publish(&mut body, 0x1002, 2)?;
                body.return_(
                    before_carry
                        .unsigned()
                        .extend::<I64>()
                        .or(before_direction.unsigned().extend::<I64>().shl(1))
                        .or(constant_carry.unsigned().extend::<I64>().shl(2))
                        .or(constant_direction.unsigned().extend::<I64>().shl(3))
                        .or(after_carry.unsigned().extend::<I64>().shl(4))
                        .or(after_direction.unsigned().extend::<I64>().shl(5)),
                )
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    let module = TestModule::new(&CompiledModule {
        segment_profile: None,
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    });
    for old_direction in [0xfe, 0xff] {
        let mut initial = initial_cpu();
        initial.flags.bytes.df = old_direction;
        for (carry_input, carry) in [(0, 0), (1, 1), (0x80, 0), (0x81, 1)] {
            for (direction_input, direction) in [(0, 0), (1, 1), (0x80, 0), (0x81, 1)] {
                let mut expected = initial;
                expected.flags.status_source.kind = 0;
                expected.flags.bytes = FlagBytes {
                    cf: carry,
                    pf: 1,
                    af: 1,
                    zf: 0,
                    sf: 1,
                    of: 0,
                    ..expected.flags.bytes
                };
                expected.flags.bytes.df = direction;
                expected.eip = 0x1002;
                expected.instruction_count = 1;
                let result = 9
                    + i64::from(old_direction & 1) * 2
                    + i64::from(carry) * 16
                    + i64::from(direction) * 32;
                assert_result(
                    &module,
                    &initial,
                    &[carry_input, direction_input],
                    &expected,
                    result,
                );
            }
        }
    }
}

#[test]
fn direction_changes_and_status_queries_retain_a_local_subtraction_recipe() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I64],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                state.write_flags(&mut body, ArithmeticOp::Subtract.apply::<I8>(0x80, 1).flags)?;
                let before = state.condition(&mut body, Condition::L)?;
                state.write_flag(&mut body, Flag::DF, true)?;
                let carry = state.read_flag(&mut body, Flag::CF)?;
                let zero = state.condition(&mut body, Condition::E)?;
                let auxiliary = state.read_flag(&mut body, Flag::AF)?;
                let after = state.condition(&mut body, Condition::L)?;
                state.publish(&mut body, 0x1002, 2)?;
                body.return_(
                    before
                        .unsigned()
                        .extend::<I64>()
                        .or(carry.unsigned().extend::<I64>().shl(1))
                        .or(zero.unsigned().extend::<I64>().shl(2))
                        .or(auxiliary.unsigned().extend::<I64>().shl(3))
                        .or(after.unsigned().extend::<I64>().shl(4)),
                )
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    let module = TestModule::new(&CompiledModule {
        segment_profile: None,
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    });
    let initial = initial_cpu();
    let mut expected = initial;
    expected.flags.status_source.kind = 1;
    expected.flags.status_source.left = 0x80;
    expected.flags.status_source.right = 1;
    expected.flags.bytes.df = 1;
    expected.eip = 0x1002;
    expected.instruction_count = 1;
    assert_result(&module, &initial, &[], &expected, 25);
}

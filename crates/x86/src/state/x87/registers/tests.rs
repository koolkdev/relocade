use super::*;
use crate::{
    state::Cpu,
    test_step::{Argument, Event, Input, Observation, Outcome, Snapshot, TestModule},
    x87::ExtendedBits,
    CompiledModule, CpuState,
};
use relocade_compiler::{Program, I64};
use wasmparser::{Operator, Parser, Payload};

fn module(
    build: impl FnOnce(&mut BlockBuilder<'_>, &mut Registers) -> Result<Val<I64>, BuildError>,
) -> CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function::<fn(u32, bool) -> u64>(|mut body, _| {
            let result = build(&mut body, &mut Registers::new(cpu.memory()))?;
            body.return_(result)
        })
        .unwrap();
    program.export("run", function).unwrap();
    CompiledModule {
        execution_target: None,
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

fn value(significand: u64, sign_exponent: u32) -> ExtendedValue {
    ExtendedValue::from_bits(ExtendedBits {
        significand: significand.into(),
        sign_exponent: sign_exponent.into(),
    })
}

fn operators(module: &CompiledModule) -> impl Iterator<Item = Operator<'_>> {
    Parser::new(0)
        .parse_all(&module.bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(body.get_operators_reader().unwrap()),
            _ => None,
        })
        .flat_map(|reader| reader.into_iter().map(Result::unwrap))
}

fn check(
    module: &TestModule,
    initial: &CpuState,
    index: usize,
    enabled: bool,
    expected: &CpuState,
    result: u64,
) {
    let input = Input {
        arguments: vec![
            Argument::I32(index as i32),
            Argument::I32(i32::from(enabled)),
        ],
        ..Input::new(&initial.to_bytes())
    };
    assert_eq!(
        module.observe(&input, 1),
        Observation {
            events: vec![Event::Return {
                outcome: Outcome::Returned(vec![Argument::I64(result as i64)]),
                snapshot: Snapshot {
                    cpu: expected.to_bytes().to_vec(),
                    guest: None
                }
            }],
            guest_unchanged: true,
            machine_unchanged: true,
        },
        "TOP {}, index {index}, enabled {enabled}",
        initial.x87.status.top
    );
}

#[test]
fn known_stack_and_physical_aliases_share_values_across_top_and_tag_changes() {
    let compiled = module(|body, registers| {
        registers.reset(body, 0)?;
        let physical = registers.physical(body, RegisterCode::from_code(0))?;
        registers.write_value(body, &physical, &value(0x1234, 0xffff), &true.into())?;
        registers.advance(body, 1, true)?;
        let stack = registers.slot(body, 7.into())?;
        let retained = registers.read(body, &stack)?;
        registers.write(body, &stack, &retained, 2.into(), &true.into())?;
        registers.reset(body, 0xffff)?;
        registers.discard_write_guards(body)?;
        let physical = registers.physical(body, RegisterCode::from_code(0))?;
        let result = registers.read(body, &physical)?.bits().significand;
        registers.publish(body)?;
        Ok(result)
    });
    assert!(!operators(&compiled)
        .any(|op| matches!(op, Operator::I64Load { .. } | Operator::I32Load16U { .. })));
    assert_eq!(
        operators(&compiled)
            .filter(|op| matches!(op, Operator::I64Store { .. }))
            .count(),
        1
    );
    let module = TestModule::new(&compiled);
    let initial = CpuState::filled(0xa5);
    let mut expected = initial;
    expected.x87.status.top = 0;
    expected.x87.tag_word = 0xffff;
    expected.x87.registers[0].significand = 0x1234;
    expected.x87.registers[0].sign_exponent = 0xffff;
    check(&module, &initial, 0, true, &expected, 0x1234);
}

#[test]
fn stack_movement_reuses_the_register_address_and_preserves_suppressed_top() {
    let compiled = module(|body, registers| {
        let enabled = body.parameter::<I1>(1)?;
        let slot = registers.slot(body, 7.into())?;
        registers.write(body, &slot, &value(0x1234, 0x4567), 2.into(), &enabled)?;
        registers.advance(body, -1, &enabled)?;
        registers.publish(body)?;
        Ok(0_u64.into())
    });
    // The payload address, tag and new TOP share one stack-position calculation.
    assert_eq!(
        operators(&compiled)
            .filter(|op| matches!(op, Operator::I32Add))
            .count(),
        1
    );
    let module = TestModule::new(&compiled);
    for top in 0..8 {
        let mut initial = CpuState::filled(0xa5);
        initial.x87.status.top = 0xa0 | top;
        for enabled in [false, true] {
            let mut expected = initial;
            if enabled {
                let target = usize::from(top.wrapping_sub(1) & 7);
                expected.x87.status.top = target as u8;
                expected.x87.registers[target].significand = 0x1234;
                expected.x87.registers[target].sign_exponent = 0x4567;
                expected.x87.tag_word =
                    (initial.x87.tag_word & !(3 << (target * 2))) | (2 << (target * 2));
            }
            check(&module, &initial, 0, enabled, &expected, 0);
        }
    }
}

#[test]
fn indexed_guarded_writes_publish_without_reading_the_old_payload() {
    let compiled = module(|body, registers| {
        let index = body.parameter::<I32>(0)?;
        let enabled = body.parameter::<I1>(1)?;
        let slot = registers.physical(body, RegisterCode::indexed(index))?;
        registers.write_value(body, &slot, &value(0x1234, 0x4567), &enabled)?;
        registers.publish(body)?;
        Ok(0_u64.into())
    });
    assert!(!operators(&compiled).any(|op| matches!(
        op,
        Operator::I64Load { .. } | Operator::I32Load16U { .. } | Operator::Select
    )));
    assert!(operators(&compiled).any(|op| matches!(op, Operator::If { .. })));
    let module = TestModule::new(&compiled);
    let initial = CpuState::filled(0xa5);
    for index in 0..8 {
        for enabled in [false, true] {
            let mut expected = initial;
            if enabled {
                expected.x87.registers[index].significand = 0x1234;
                expected.x87.registers[index].sign_exponent = 0x4567;
            }
            check(&module, &initial, index, enabled, &expected, 0);
        }
    }
}

#[test]
fn uncertain_aliases_preserve_guarded_writes_and_captured_reads() {
    let compiled = module(|body, registers| {
        let index = body.parameter::<I32>(0)?;
        let enabled = body.parameter::<I1>(1)?;
        let stack = registers.slot(body, 0.into())?;
        let old = registers.read(body, &stack)?.bits().significand;
        registers.write(body, &stack, &value(0x1234, 0x4567), 2.into(), &enabled)?;
        let physical = registers.physical(body, RegisterCode::indexed(index))?;
        let middle = registers.read(body, &physical)?.bits().significand;
        registers.write_value(body, &physical, &value(0x5678, 0xffff), &true.into())?;
        let stack = registers.slot(body, 0.into())?;
        let current = registers.read(body, &stack)?.bits().significand;
        registers.publish(body)?;
        Ok(old.shl(32).or(middle.shl(16)).or(current))
    });
    let module = TestModule::new(&compiled);
    for top in 0..8 {
        let mut initial = CpuState::filled(0xa5);
        initial.x87.status.top = top as u8;
        for (index, register) in initial.x87.registers.iter_mut().enumerate() {
            register.significand = 0x100 + index as u64;
        }
        for index in 0..8 {
            for enabled in [false, true] {
                let mut expected = initial;
                if enabled {
                    expected.x87.registers[top].significand = 0x1234;
                    expected.x87.registers[top].sign_exponent = 0x4567;
                    expected.x87.tag_word =
                        (initial.x87.tag_word & !(3 << (top * 2))) | (2 << (top * 2));
                }
                let middle = expected.x87.registers[index].significand;
                expected.x87.registers[index].significand = 0x5678;
                expected.x87.registers[index].sign_exponent = 0xffff;
                let result = (initial.x87.registers[top].significand << 32)
                    | (middle << 16)
                    | expected.x87.registers[top].significand;
                check(&module, &initial, index, enabled, &expected, result);
            }
        }
    }
}

use super::*;
use crate::{
    state::Cpu,
    test_step::{Argument, Event, Input, Observation, Outcome, Snapshot, TestModule},
    x87::ExtendedBits,
    CompiledModule, CpuState,
};
use wasm86_compiler::{Program, Signature, Type, I64};
use wasmparser::{Operator, Parser, Payload};

fn module(
    build: impl FnOnce(&mut BlockBuilder<'_>, &mut Registers) -> Result<Val<I64>, BuildError>,
) -> CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32, Type::I1],
                results: vec![Type::I64],
            },
            |mut body| {
                let result = build(&mut body, &mut Registers::new(cpu.memory()))?;
                body.return_(result)
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    CompiledModule {
        execution_profile: None,
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

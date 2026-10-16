use crate::state::SegmentState;
use crate::{
    memory::Intent,
    segment::{Segment, SegmentSelection},
    state::{exit, Cpu},
    CompiledModule, ExecutionProfile, SegmentProfile,
};
use wasm86_compiler::{Program, Signature, Type, Val, I32, I64};
use wasmparser::{Operator, Parser, Payload, Validator};

mod ranges;
mod runtime;

fn translation(
    profile: ExecutionProfile,
    intent: Intent,
    selection: Option<fn(Val<I32>) -> SegmentSelection>,
) -> CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32, Type::I32],
                results: vec![Type::I64],
            },
            |mut body| {
                let mut access = SegmentState::new(&cpu, profile);
                let choice = body.parameter::<I32>(0)?;
                let offset = body.parameter::<I32>(1)?;
                let linear = match selection {
                    Some(selection) => access.translate(
                        &mut body,
                        &selection(choice),
                        &offset,
                        4,
                        intent,
                        exit::exception,
                    )?,
                    None => offset,
                };
                body.return_(linear.unsigned().extend::<I64>())
            },
        )
        .unwrap();
    program.export("translate", function).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    CompiledModule {
        bytes,
        entry: "translate".into(),
        execution_target: Some(crate::Compiler::new(profile).target()),
    }
}

fn default_segment(choice: Val<I32>) -> SegmentSelection {
    SegmentSelection::Indexed(choice.eq(0).select(Segment::Ss as u32, Segment::Ds as u32))
}

fn function_bodies(bytes: &[u8]) -> Vec<&[u8]> {
    Parser::new(0)
        .parse_all(bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(body.as_bytes()),
            _ => None,
        })
        .collect()
}

#[test]
fn flat_data_segments_and_address_defaults_generate_only_the_effective_offset() {
    for intent in [Intent::Read, Intent::Write, Intent::ReadOrExecute] {
        let expected = translation(
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            intent,
            None,
        );
        for selection in [
            (|_| Segment::Ds.into()) as fn(Val<I32>) -> SegmentSelection,
            |_| Segment::Es.into(),
            |_| Segment::Ss.into(),
            default_segment,
            |_| SegmentSelection::Indexed((Segment::Ds as u32).into()),
            |choice| {
                SegmentSelection::Indexed(choice.eq(0).select(
                    Segment::Ss as u32,
                    choice.eq(1).select(Segment::Ds as u32, Segment::Es as u32),
                ))
            },
        ] {
            let actual = translation(
                ExecutionProfile::Protected(SegmentProfile::Flat32),
                intent,
                Some(selection),
            );
            assert_eq!(
                function_bodies(&actual.bytes),
                function_bodies(&expected.bytes)
            );
        }
    }
}

#[test]
fn address_defaults_read_only_their_dynamic_fields() {
    for (profile, expected_loads) in [
        (ExecutionProfile::Real16, (2, 0)),
        (SegmentProfile::Segmented16.into(), (4, 2)),
        (SegmentProfile::Segmented32.into(), (4, 2)),
    ] {
        for intent in [Intent::Read, Intent::Write] {
            let actual = translation(profile, intent, Some(default_segment));
            let mut loads = (0, 0);
            for payload in Parser::new(0).parse_all(&actual.bytes) {
                if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                    for op in body.get_operators_reader().unwrap() {
                        match op.unwrap() {
                            Operator::I32Load { .. } => loads.0 += 1,
                            Operator::I32Load16U { .. } => loads.1 += 1,
                            _ => {}
                        }
                    }
                }
            }
            assert_eq!(loads, expected_loads);
        }
    }
}

#[test]
fn unknown_indices_generate_only_the_raw_cache_reads() {
    fn cache_read(refine: bool) -> Vec<u8> {
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        let function = program
            .function(
                Signature {
                    parameters: vec![Type::I32],
                    results: vec![Type::I32, Type::I32, Type::I16, Type::I16],
                },
                |mut body| {
                    let mut access = SegmentState::new(&cpu, SegmentProfile::Flat32.into());
                    let index = SegmentSelection::Indexed(body.parameter::<I32>(0)?);
                    let cache = if refine {
                        access.read(&mut body, &index)?
                    } else {
                        cpu.read_segment(&mut body, &index)?
                    };
                    body.return_((cache.base, cache.limit, cache.selector, cache.attributes))
                },
            )
            .unwrap();
        program.export("read", function).unwrap();
        program.compile().unwrap()
    }
    assert_eq!(
        function_bodies(&cache_read(true)),
        function_bodies(&cache_read(false))
    );
}

#[test]
fn nested_reads_reuse_previously_loaded_segment_fields() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32],
                results: vec![Type::I32, Type::I32],
            },
            |mut body| {
                let mut access = SegmentState::new(&cpu, ExecutionProfile::Real16);
                let first = access.read(&mut body, &Segment::Ds.into())?.base;
                let condition = body.parameter::<I32>(0)?.eq(0);
                body.if_(condition, |mut arm| {
                    let mut nested = access.clone();
                    let second = nested.read(&mut arm, &Segment::Ds.into())?.base;
                    arm.return_((&first, second.add(1)))
                })?;
                let second = access.read(&mut body, &Segment::Ds.into())?.base;
                body.return_((first, second.add(2)))
            },
        )
        .unwrap();
    program.export("read", function).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let loads = Parser::new(0)
        .parse_all(&bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(
                body.get_operators_reader()
                    .unwrap()
                    .into_iter()
                    .filter(|op| matches!(op, Ok(Operator::I32Load { .. })))
                    .count(),
            ),
            _ => None,
        })
        .sum::<usize>();
    assert_eq!(loads, 1);
}

#[test]
fn flat_code_reads_and_fetches_generate_only_the_effective_offset() {
    for intent in [Intent::Read, Intent::Fetch, Intent::ReadOrExecute] {
        let actual = translation(
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            intent,
            Some(|_| Segment::Cs.into()),
        );
        let expected = translation(
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            intent,
            None,
        );
        assert_eq!(
            function_bodies(&actual.bytes),
            function_bodies(&expected.bytes)
        );
    }
}

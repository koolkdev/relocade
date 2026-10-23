use crate::state::SegmentReads;
use crate::test_step::AccessResult;
use crate::{
    memory::Intent,
    segment::{Segment, SegmentSelection},
    state::Cpu,
    CompiledModule, ExecutionProfile, SegmentProfile,
};
use relocade_compiler::{Program, Signature, Type, Val, I32, I64};
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
                results: relocade_compiler::abi::Function::<fn() -> AccessResult>::new(
                    "test", "access",
                )
                .signature()
                .results,
            },
            |mut body| {
                let mut access = SegmentReads::new(&cpu, profile);
                let choice = body.parameter::<I32>(0)?;
                let offset = body.parameter::<I32>(1)?;
                let linear = match selection {
                    Some(selection) => access.translate(
                        &mut body,
                        &selection(choice),
                        &offset,
                        4,
                        intent,
                        AccessResult::fault,
                    )?,
                    None => offset,
                };
                AccessResult::value(body, linear.unsigned().extend::<I64>())
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
    SegmentSelection::ds_or_ss(choice.eq(0))
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
            |_| SegmentSelection::indexed((Segment::Ds as u32).into()),
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
fn address_defaults_load_each_dynamic_field_once() {
    for (profile, expected_loads) in [
        (ExecutionProfile::Real16, (1, 0)),
        (SegmentProfile::Segmented16.into(), (2, 4)),
        (SegmentProfile::Segmented32.into(), (2, 4)),
    ] {
        for intent in [Intent::Read, Intent::Write] {
            let actual = translation(profile, intent, Some(default_segment));
            let mut loads = (0, 0);
            for payload in Parser::new(0).parse_all(&actual.bytes) {
                if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                    for op in body.get_operators_reader().unwrap() {
                        match op.unwrap() {
                            Operator::I32Load { .. } => loads.0 += 1,
                            Operator::I32Load8U { .. } => loads.1 += 1,
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
    fn cache_read() -> Vec<u8> {
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        let function = program
            .function(
                Signature {
                    parameters: vec![Type::I32],
                    results: vec![
                        Type::I32,
                        Type::I32,
                        Type::I16,
                        Type::I1,
                        Type::I1,
                        Type::I1,
                        Type::I1,
                        Type::I1,
                        Type::I1,
                    ],
                },
                |mut body| {
                    let mut access = SegmentReads::new(&cpu, SegmentProfile::Flat32.into());
                    let index = SegmentSelection::indexed(body.parameter::<I32>(0)?);

                    let base = access.base(&mut body, &index)?;
                    let limit = access.limit(&mut body, &index)?;
                    let selector = access.read_selector(&mut body, &index)?;
                    let attributes = [
                        access.usable(&mut body, &index)?,
                        access.readable(&mut body, &index)?,
                        access.writable(&mut body, &index)?,
                        access.executable(&mut body, &index)?,
                        access.expand_down(&mut body, &index)?,
                        access.big(&mut body, &index)?,
                    ];
                    body.return_((base, limit, selector, attributes))
                },
            )
            .unwrap();
        program.export("read", function).unwrap();
        program.compile().unwrap()
    }
    let bytes = cache_read();
    let mut loads = (0, 0, 0);
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for op in body.get_operators_reader().unwrap() {
                match op.unwrap() {
                    Operator::I32Load { .. } => loads.0 += 1,
                    Operator::I32Load16U { .. } => loads.1 += 1,
                    Operator::I32Load8U { .. } => loads.2 += 1,
                    Operator::If { .. } | Operator::Select => {
                        panic!("unknown indices need no selection branches")
                    }
                    _ => {}
                }
            }
        }
    }
    // Base, limit, selector, and the six permission/size fields.
    assert_eq!(loads, (2, 1, 6));
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
                let mut access = SegmentReads::new(&cpu, ExecutionProfile::Real16);
                let first = access.base(&mut body, &Segment::Ds.into())?;
                let condition = body.parameter::<I32>(0)?.eq(0);
                body.if_(condition, |mut arm| {
                    let mut nested = access.clone();
                    let second = nested.base(&mut arm, &Segment::Ds.into())?;
                    arm.return_((&first, second.add(1)))
                })?;
                let second = access.base(&mut body, &Segment::Ds.into())?;
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

use super::SegmentAccess;
use crate::{
    memory::Intent,
    segment::{Segment, SegmentProfile, SegmentSelection},
    state::{exit, Cpu},
    CompiledModule,
};
use wasm86_compiler::{Program, Signature, Type, Val, I32, I64};
use wasmparser::Validator;

mod runtime;

fn translation(
    profile: SegmentProfile,
    intent: Intent,
    selection: Option<fn(Val<I32>) -> SegmentSelection>,
) -> CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let access = SegmentAccess::new(&cpu, profile);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32, Type::I32],
                results: vec![Type::I64],
            },
            |mut body| {
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
        segment_profile: Some(profile),
    }
}

fn default_segment(choice: Val<I32>) -> SegmentSelection {
    SegmentSelection::AddressDefault(choice.eq(0).select(Segment::Ss as u32, Segment::Ds as u32))
}

#[test]
fn flat_data_segments_and_address_defaults_generate_only_the_effective_offset() {
    for intent in [Intent::Read, Intent::Write] {
        let expected = translation(SegmentProfile::Flat32, intent, None);
        for selection in [
            (|_| Segment::Ds.into()) as fn(Val<I32>) -> SegmentSelection,
            |_| Segment::Es.into(),
            |_| Segment::Ss.into(),
            default_segment,
        ] {
            let actual = translation(SegmentProfile::Flat32, intent, Some(selection));
            assert_eq!(actual.bytes, expected.bytes);
        }
    }
}

#[test]
fn flat_code_reads_and_fetches_generate_only_the_effective_offset() {
    for intent in [Intent::Read, Intent::Fetch] {
        assert_eq!(
            translation(SegmentProfile::Flat32, intent, Some(|_| Segment::Cs.into())).bytes,
            translation(SegmentProfile::Flat32, intent, None).bytes,
        );
    }
}

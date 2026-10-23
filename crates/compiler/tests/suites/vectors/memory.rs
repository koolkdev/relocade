//! SIMD memory forms retain the original accesses and shared scalar uses.

use super::*;
use relocade_compiler::{I16, I64, V64};
use wasmparser::{Operator, Parser, Payload};

fn operators(bytes: &[u8]) -> Vec<Operator<'_>> {
    Parser::new(0)
        .parse_all(bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(body.get_operators_reader().unwrap()),
            _ => None,
        })
        .flat_map(|reader| reader.into_iter().map(Result::unwrap))
        .collect()
}

fn snapshot_across_call() {
    let mut initial = vec![0xa5; 65536];
    initial[65528..].copy_from_slice(&0xffff_0001_7fff_8000_u64.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let overwrite = fixture.program.function::<fn()>(move |mut body, ()| {
        body.store::<I64>(memory, 65528, 0_u64)?;
        body.return_(())
    });
    let module = fixture.function(&[], &[], move |mut body| {
        let old = body.load::<I64>(memory, 65528)?;
        body.call::<()>(overwrite, &[])?;
        let packed = Val::<V64>::from_bits(old).add_lanes::<I16>(0x0001_0001_0001_0001_u64);
        body.store(memory, 16, packed.to_bits())?;
        body.return_(())
    });
    let ops = operators(module.bytes());
    let load = ops
        .iter()
        .position(|op| matches!(op, Operator::V128Load64Splat { memarg } if memarg.offset == 65528))
        .unwrap();
    let call = ops
        .iter()
        .position(|op| matches!(op, Operator::Call { .. }))
        .unwrap();
    assert!(load < call);
    assert!(ops.iter().any(
        |op| matches!(op, Operator::V128Store64Lane { memarg, lane: 0 } if memarg.offset == 16)
    ));
    assert!(!ops
        .iter()
        .any(|op| matches!(op, Operator::I64x2Splat | Operator::I64x2ExtractLane { .. })));
    let mut expected = initial.clone();
    expected[65528..].fill(0);
    expected[16..24].copy_from_slice(&0x0000_0002_8000_8001_u64.to_le_bytes());

    let mut instance = module.instantiate();
    instance.call::<()>(()).unwrap();
    assert_eq!(instance.memory("state"), expected);
}

fn shared_scalar_read() {
    let initial = 0xffff_0001_7fff_8000_u64.to_le_bytes();
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[], &[Type::I64; 2], move |mut body| {
        let scalar = body.load::<I64>(memory, 0)?;
        let packed = Val::<V64>::from_bits(&scalar).add_lanes::<I16>(0x0001_0001_0001_0001_u64);
        body.return_((scalar, packed.to_bits()))
    });
    let ops = operators(module.bytes());
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, Operator::I64Load { .. }))
            .count(),
        1
    );
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, Operator::I64x2Splat))
            .count(),
        1
    );
    assert!(!ops
        .iter()
        .any(|op| matches!(op, Operator::V128Load64Splat { .. })));
    let expected = [
        Value::I64(0xffff_0001_7fff_8000_u64 as i64),
        Value::I64(0x0000_0002_8000_8001),
    ];

    assert_eq!(
        module.instantiate().call_values("run", &[]).unwrap(),
        expected
    );
}

fn lane_stores() {
    let mut initial = vec![0xa5; 64];
    initial[..16].copy_from_slice(&LEFT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[], &[Type::I16], move |mut body| {
        let vector = body.load::<V128>(memory, 0)?;
        body.store(memory, 16, vector.extract_lane::<I8>(13))?;
        let shared = vector.extract_lane::<I16>(5);
        body.store(memory, 18, &shared)?;
        body.store(memory, 24, vector.extract_lane::<I32>(3))?;
        let high = vector.extract_lane::<I64>(1);
        body.store(memory, 32, &high)?;
        body.store(memory, 48, high)?;
        // Narrowing an I32 lane changes the store's lane numbering; leave this
        // extraction intact instead of selecting byte lane 2.
        body.store(memory, 40, vector.extract_lane::<I32>(2).truncate::<I8>())?;
        body.return_(shared)
    });
    let ops = operators(module.bytes());
    let stores: Vec<_> = ops
        .iter()
        .filter_map(|op| match op {
            Operator::V128Store8Lane { memarg, lane } => Some((1, *lane, memarg.offset)),
            Operator::V128Store16Lane { memarg, lane } => Some((2, *lane, memarg.offset)),
            Operator::V128Store32Lane { memarg, lane } => Some((4, *lane, memarg.offset)),
            Operator::V128Store64Lane { memarg, lane } => Some((8, *lane, memarg.offset)),
            _ => None,
        })
        .collect();
    assert_eq!(
        stores,
        [(1, 13, 16), (2, 5, 18), (4, 3, 24), (8, 1, 32), (8, 1, 48)]
    );
    assert!(ops
        .iter()
        .any(|op| matches!(op, Operator::I16x8ExtractLaneU { lane: 5 })));
    let mut expected = initial.clone();
    expected[16] = 0x45;
    expected[18..20].copy_from_slice(&[0xab, 0x89]);
    expected[24..28].copy_from_slice(&[0x67, 0x45, 0x23, 0x01]);
    expected[32..40].copy_from_slice(&0x0123_4567_89ab_cdef_u64.to_le_bytes());
    expected[48..56].copy_from_slice(&0x0123_4567_89ab_cdef_u64.to_le_bytes());
    expected[40] = 0xef;
    let returned = [Value::I32(0x89ab)];

    let mut instance = module.instantiate();
    assert_eq!(instance.call_values("run", &[]).unwrap(), returned);
    assert_eq!(&instance.memory("state")[..64], expected);
}

#[test]
fn vector_memory_selection_preserves_snapshots_widths_and_shared_values() {
    snapshot_across_call();
    shared_scalar_read();
    lane_stores();
}

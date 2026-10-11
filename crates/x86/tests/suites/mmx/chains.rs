//! Packed SSA chains retain scalar publication and precise fault boundaries.

use super::*;
use wasm86_x86::compile_block_from_bytes;
use wasmparser::{Operator, Parser, Payload};

const MIXED: [u8; 9] = [
    0x0f, 0x74, 0xc1, // PCMPEQB mm0,mm1
    0x0f, 0xfc, 0xc2, // PADDB mm0,mm2
    0x0f, 0xf9, 0xc3, // PSUBW mm0,mm3
];

fn chains(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let code = [0x0f, 0xfd, 0xc1].repeat(8);
    let mut initial = image(&code);
    write_register_bits(&mut initial.cpu, 0, (0x7ffe_fffe_8000_0001, 0x1234));
    write_register_bits(&mut initial.cpu, 1, (0x0003_0002_0001_ffff, 0x5678));
    let mut cpu = initial.cpu;
    let mut steps = Vec::new();
    for result in [
        0x8001_0000_8001_0000,
        0x8004_0002_8002_ffff,
        0x8007_0004_8003_fffe,
        0x800a_0006_8004_fffd,
        0x800d_0008_8005_fffc,
        0x8010_000a_8006_fffb,
        0x8013_000c_8007_fffa,
        0x8016_000e_8008_fff9,
    ] {
        cpu = complete(cpu, 3);
        write_register_bits(&mut cpu, 0, (result, 0xffff));
        steps.push(dispatch(cpu));
    }
    checks.check(
        "packed chain preserves per-lane wrapping and MMX state",
        &code,
        &initial,
        &steps,
    );

    // The load faults after the mixed-width packed results have completed.
    let mut code = MIXED.to_vec();
    code.extend_from_slice(&[0x0f, 0x6f, 0x20]); // MOVQ mm4,[eax]
    let mut initial = image(&code);
    initial.cpu.registers.eax = 0x4000;
    for (register, value) in [
        0x557f_8001_00ff_7f80,
        0x547f_8001_ff00_807f,
        0x0102_0304_0506_0708,
        0x0001_0002_0003_0004,
    ]
    .into_iter()
    .enumerate()
    {
        write_register_bits(&mut initial.cpu, register, (value, 0x1234));
    }
    let mut cpu = initial.cpu;
    let mut steps = Vec::new();
    for result in [
        0x00ff_ffff_0000_0000,
        0x0101_0203_0506_0708,
        0x0100_0201_0503_0704,
    ] {
        cpu = complete(cpu, 3);
        write_register_bits(&mut cpu, 0, (result, 0xffff));
        steps.push(dispatch(cpu));
    }
    steps.push(Step {
        cpu,
        ram: &[],
        exit: Exit::PageFault {
            address: 0x4000,
            error: 0,
        },
    });
    checks.check(
        "mixed packed lanes publish completed work at a later fault",
        &code,
        &initial,
        &steps,
    );
}

test_frontends!(mmx_packed_chains, chains);

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

#[test]
fn packed_add_chains_only_convert_at_scalar_boundaries() {
    let code = [0x0f, 0xfd, 0xc1].repeat(8);
    let inputs = 2;
    let module = compile_block_from_bytes(0x1000, &code, (code.len() / 3) as u32).unwrap();
    let ops = operators(&module.bytes);
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, Operator::I64x2ReplaceLane { .. }))
            .count(),
        inputs
    );
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, Operator::I64x2ExtractLane { .. }))
            .count(),
        1
    );
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, Operator::I64Load { .. }))
            .count(),
        inputs
    );
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, Operator::I64Store { .. }))
            .count(),
        1
    );
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, Operator::I32Load8U { memarg } if memarg.offset == 192))
            .count(),
        1
    );
}

#[test]
fn packed_self_identities_need_no_payload_load() {
    for opcode in [0xf9, 0x75, 0x65] {
        // PSUBW, PCMPEQW, PCMPGTW mm0,mm0
        let module = compile_block_from_bytes(0x1000, &[0x0f, opcode, 0xc0], 1).unwrap();
        let ops = operators(&module.bytes);
        assert!(!ops.iter().any(|op| matches!(
            op,
            Operator::I64Load { .. } | Operator::I16x8Sub | Operator::I16x8Eq | Operator::I16x8GtS
        )));
        // Constant payloads still publish the MMX destination's exponent.
        assert!(ops
            .iter()
            .any(|op| matches!(op, Operator::I32Store16 { memarg } if memarg.offset == 208)));
    }
}

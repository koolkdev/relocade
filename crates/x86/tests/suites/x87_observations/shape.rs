//! Observations add guards only to their consumers, once per unchanged value.
use super::*;
use wasm86_x86::{compile_block_from_bytes_with_profile, CompiledModule};
use wasmparser::{Operator, Parser, Payload, TypeRef};

fn operators(module: &CompiledModule) -> impl Iterator<Item = Operator<'_>> {
    Parser::new(0)
        .parse_all(&module.bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(body.get_operators_reader().unwrap()),
            _ => None,
        })
        .flat_map(|reader| reader.into_iter().map(Result::unwrap))
}

fn interpreter_calls(module: &CompiledModule) -> usize {
    let mut function = 0;
    for payload in Parser::new(0).parse_all(&module.bytes) {
        if let Payload::ImportSection(imports) = payload.unwrap() {
            for import in imports {
                let import = import.unwrap();
                if matches!(import.ty, TypeRef::Func(_)) {
                    if import.module == "wasm86" && import.name == "interpret" {
                        return operators(module).filter(|op| matches!(op, Operator::ReturnCall { function_index } if *function_index == function)).count();
                    }
                    function += 1;
                }
            }
        }
    }
    0
}

#[test]
fn blocks_without_mode_consumers_are_byte_identical() {
    let cases: &[&[u8]] = &[
        &[0xb8, 7, 0, 0, 0],          // MOV EAX, 7
        &[0xd9, 0x05, 0, 0x40, 0, 0], // FLD m32
        &[0xdf, 0x2d, 0, 0x40, 0, 0], // FILD m64
        &[0xdb, 0x3d, 0, 0x40, 0, 0], // FSTP m80
        &[0xdd, 0xd9],                // FSTP ST1
        &[0xd9, 0x3d, 0, 0x40, 0, 0], // FNSTCW
    ];
    for &code in cases {
        let ordinary =
            compile_block_from_bytes_with_profile(0x1000, code, 1, SegmentProfile::Flat32).unwrap();
        let observed = masked_precision_compiler(0, 2)
            .compile_block(0x1000, code, 1)
            .unwrap();
        assert!(ordinary.bytes == observed.bytes, "{code:x?}");
    }
}

#[test]
fn unchanged_modes_add_one_guard_to_an_arithmetic_sequence() {
    for pc in 0..4 {
        for rc in 0..4 {
            for opcode in [0xc1, 0xc9, 0xe1, 0xf1] {
                for count in [1, 8] {
                    let module = compiler(pc, rc)
                        .compile_block(0x1000, &[0xd8, opcode].repeat(count), count as u32)
                        .unwrap();
                    // One mode guard, one operand guard, and one result guard per operation.
                    assert_eq!(interpreter_calls(&module), count + 2);
                    for offset in [158, 159] {
                        assert_eq!(operators(&module).filter(|op| matches!(op, Operator::I32Load8U { memarg } if memarg.offset == offset)).count(), 1);
                    }
                }
            }
        }
    }
}

#[test]
fn known_controls_need_no_mode_guard() {
    let code = [0xdb, 0xe3, 0xd9, 0x05, 0, 0x40, 0, 0, 0xd8, 0xc8]; // FNINIT; FLD m32; FMUL ST0, ST0
    let ordinary = Compiler::new(SegmentProfile::Flat32)
        .compile_block(0x1000, &code, 3)
        .unwrap();
    let observed = compiler(3, 0).compile_block(0x1000, &code, 3).unwrap();
    assert_eq!(interpreter_calls(&ordinary), interpreter_calls(&observed));
    assert_eq!(
        operators(&ordinary)
            .filter(|op| matches!(op, Operator::If { .. }))
            .count(),
        operators(&observed)
            .filter(|op| matches!(op, Operator::If { .. }))
            .count()
    );
    assert!(!operators(&observed).any(
        |op| matches!(op, Operator::I32Load8U { memarg } if matches!(memarg.offset, 158 | 159))
    ));
}

#[test]
fn only_masked_set_precision_adds_status_guards() {
    let code = [0xd8, 0xc9].repeat(8);
    let modes_only = compiler(3, 0).compile_block(0x1000, &code, 8).unwrap();
    for (mask, flag) in [(0x80, 0x80), (0x80, 0x81), (0x81, 0x80)] {
        let mut observed = observed_cpu(3, 0);
        observed.x87.control.precision_mask = mask;
        observed.x87.status.precision = flag;
        let module = Compiler::new(SegmentProfile::Flat32)
            .specialize_on_cpu(&observed)
            .compile_block(0x1000, &code, 8)
            .unwrap();
        assert!(module.bytes == modes_only.bytes);
    }
}

#[test]
fn masked_set_precision_reuses_its_guard_and_original_byte() {
    for pc in [2, 3] {
        for opcode in [0xc1, 0xc9, 0xe1, 0xf1] {
            let count = 8;
            let module = masked_precision_compiler(pc, 0)
                .compile_block(0x1000, &[0xd8, opcode].repeat(count), count as u32)
                .unwrap();
            // PM and PE join the existing mode guard; they do not add a guard
            // per instruction. Unchanged controls and status are read once.
            assert_eq!(interpreter_calls(&module), count + 2);
            let ops: Vec<_> = operators(&module).collect();
            for offset in [157, 158, 159, 185] {
                assert_eq!(ops.iter().filter(|op| matches!(op, Operator::I32Load8U { memarg } if memarg.offset == offset)).count(), 1);
            }
            let pe_load = ops
                .iter()
                .position(|op| matches!(op, Operator::I32Load8U { memarg } if memarg.offset == 185))
                .unwrap();
            let Operator::LocalSet { local_index: pe } = ops[pe_load + 1] else {
                panic!("the observed PE byte remains available through the block");
            };
            // Every publication reuses the observed byte, including its unused
            // bits. The sequence must not accumulate PE OR calculations.
            for (index, op) in ops.iter().enumerate() {
                if matches!(op, Operator::I32Store8 { memarg } if memarg.offset == 185) {
                    assert!(
                        matches!(ops[index - 1], Operator::LocalGet { local_index } if local_index == pe)
                    );
                }
            }
        }
    }
}

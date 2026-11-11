//! Admission predicates remove fixed privilege checks from generated entries.

use crate::{Compiler, ExecutionContext, SegmentProfile};
use std::mem::offset_of;
use wasmparser::{Operator, Parser, Payload};

#[test]
fn admitted_controls_remove_privilege_and_timestamp_state_reads() {
    let controls = [
        offset_of!(ExecutionContext, cpu.cr0) as u64,
        offset_of!(ExecutionContext, cpu.cr4) as u64,
        offset_of!(ExecutionContext, cpu.flags.bytes.vm) as u64,
        offset_of!(ExecutionContext, cpu.segments.cs.selector) as u64,
    ];
    for code in [&[0x0f, 0x31][..], &[0xfa]] {
        let module = Compiler::new(SegmentProfile::Flat32)
            .compile_block(0x1000, code, 1)
            .unwrap();
        for payload in Parser::new(0).parse_all(&module.bytes) {
            if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                for operator in body.get_operators_reader().unwrap() {
                    if let Operator::I32Load { memarg }
                    | Operator::I32Load8U { memarg }
                    | Operator::I32Load16U { memarg } = operator.unwrap()
                    {
                        assert!(!controls.contains(&memarg.offset), "{code:02x?}");
                    }
                }
            }
        }
    }
}

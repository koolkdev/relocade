//! Admission predicates remove fixed privilege checks from generated entries.

use crate::{
    Compiler, CpuState, ExecutionContext, SegmentDefaultSize, SegmentProfile, StateAssumption,
};
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

#[test]
fn entry_assumptions_keep_supplied_state_conditions_and_exclude_observations() {
    for (profile, cpu) in [
        (SegmentProfile::Flat32, CpuState::default()),
        (SegmentProfile::Segmented32, CpuState::default()),
        (
            SegmentProfile::Segmented16,
            CpuState::protected_mode(SegmentDefaultSize::Bits16, SegmentDefaultSize::Bits32),
        ),
    ] {
        let compiler = Compiler::new(profile);
        let block = compiler.compile_block(0x1000, &[0x90], 1).unwrap();
        assert!(cpu.satisfies(&block.state_assumptions));
        assert!(block.state_assumptions.contains(&StateAssumption {
            offset: offset_of!(CpuState, segments.cs.selector) as u32,
            bytes: 2,
            mask: 3,
            expected: 3,
        }));
        for change in [
            (|cpu: &mut CpuState| cpu.cr0 ^= 1) as fn(&mut CpuState),
            |cpu| cpu.cr4 |= 4,
            |cpu| cpu.flags.bytes.vm |= 1,
            |cpu| cpu.segments.cs.selector &= !3,
            |cpu| cpu.segments.cs.big ^= 1,
        ] {
            let mut changed = cpu;
            change(&mut changed);
            assert!(!changed.satisfies(&block.state_assumptions));
        }
        let mut changed = cpu;
        changed.flags.bytes.vm = 0xfe;
        changed.segments.cs.selector += 8;
        changed.x87.control.precision_control = 0;
        assert!(changed.satisfies(&block.state_assumptions));
        let observed = compiler
            .specialize_on_cpu(&changed)
            .compile_block(0x1000, &[0xd8, 0xc9], 1)
            .unwrap();
        assert_eq!(observed.state_assumptions, block.state_assumptions);
        changed.segments.ds.base = 0x2000;
        assert_eq!(
            changed.satisfies(&block.state_assumptions),
            profile != SegmentProfile::Flat32
        );
    }
}

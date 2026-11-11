//! A disabled target must omit budget-specific work while retaining interrupts.

use crate::{Compiler, SegmentProfile};
use wasmparser::{Operator, Parser, Payload};

#[test]
fn disabled_targets_omit_budget_loads_and_comparisons() {
    for enabled in [false, true] {
        let module = Compiler::new(SegmentProfile::Flat32)
            .with_instruction_budgets(enabled)
            .compile_block(0x1000, &[0x90, 0xeb, 0], 2)
            .unwrap();
        let (mut target, mut comparison, mut interrupt) = (false, false, false);
        for payload in Parser::new(0).parse_all(&module.bytes) {
            if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                for operator in body.get_operators_reader().unwrap() {
                    match operator.unwrap() {
                        Operator::I64Load { memarg } if memarg.offset == 552 => target = true,
                        Operator::I64GeS => comparison = true,
                        Operator::I32AtomicLoad8U { memarg } if memarg.offset == 548 => {
                            interrupt = true
                        }
                        _ => {}
                    }
                }
            }
        }
        assert_eq!((target, comparison), (enabled, enabled));
        assert!(interrupt);
    }
}

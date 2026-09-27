//! Schedule bounded multiway dispatch and each case's results.
use wasm_encoder::{BlockType, ValType};

use super::{Instruction, LocalOp, Scheduler};
use crate::{
    body::{Block, Site, SwitchCase, Target},
    integer::{BinaryOp, CompareOp},
    Expression, Type,
};

impl Scheduler<'_> {
    pub(super) fn open_switch(
        &mut self,
        cases: usize,
        result: BlockType,
        site: Site,
        outputs: &[usize],
    ) {
        // The outer block joins falling-through arms. Each inner block is a case
        // label, followed by the default label. Open them before evaluating the
        // selector so its stack value is inside the innermost label's scope.
        self.begin_control(
            Instruction::Block(result),
            Some(Target::exit(site)),
            outputs,
        );
        for _ in 0..=cases {
            self.begin_control(Instruction::Block(BlockType::Empty), None, &[]);
        }
    }

    pub(super) fn switch(&mut self, cases: &[SwitchCase], default: &Block, site: Site) {
        self.dispatch_switch(cases);
        let before_arm = self.available.clone();
        for case in cases {
            self.end_control();
            self.available.clone_from(&before_arm);
            self.block(&case.block, None);
            if case.block.terminal.is_none() {
                self.branch_to(Target::exit(site));
            }
        }
        self.end_control();
        self.available.clone_from(&before_arm);
        self.block(default, Some(Target::exit(site)));
        self.end_control();
        self.available = before_arm;
    }

    fn dispatch_switch(&mut self, cases: &[SwitchCase]) {
        let Some(first) = cases.first() else {
            self.instructions.push(Instruction::Drop);
            return;
        };
        let default = u32::try_from(cases.len()).expect("case count fits the Wasm index space");
        let span = u64::from(cases.last().unwrap().key) - u64::from(first.key) + 1;
        // Permit modest gaps (including opcode sets near six slots per key), but
        // cap tables independently of key magnitude and supplied case count.
        let dense = span <= 4096 && span <= 8 * cases.len() as u64;
        let targets = if dense {
            if first.key != 0 {
                self.instructions.push(Instruction::Constant {
                    ty: Type::I32,
                    bits: u64::from(first.key),
                });
                self.instructions.push(Instruction::Expression {
                    result_type: Type::I32,
                    expression: Expression::Binary {
                        operator: BinaryOp::Sub,
                        left: Type::I32,
                        right: Type::I32,
                    },
                });
            }
            let mut targets = vec![default; span as usize];
            for (index, case) in cases.iter().enumerate() {
                targets[(case.key - first.key) as usize] = index as u32;
            }
            targets
        } else {
            // A sparse selector is still evaluated once. Balanced comparisons
            // map it to a compact case index; no case body or default is copied.
            let selector_slot = self.temporary(ValType::I32);
            self.local(selector_slot, LocalOp::Set);
            self.sparse_case_index(cases, 0, default, selector_slot);
            (0..default).collect()
        };
        self.instructions
            .push(Instruction::BranchTable { targets, default });
    }

    fn sparse_case_index(
        &mut self,
        cases: &[SwitchCase],
        first_index: u32,
        default: u32,
        selector_slot: usize,
    ) {
        let midpoint = cases.len() / 2;
        self.local(selector_slot, LocalOp::Get);
        self.instructions.push(Instruction::Constant {
            ty: Type::I32,
            bits: u64::from(cases[midpoint].key),
        });
        self.instructions.push(Instruction::Expression {
            result_type: Type::I1,
            expression: Expression::Compare {
                operator: if cases.len() == 1 {
                    CompareOp::Eq
                } else {
                    CompareOp::LtUnsigned
                },
                left: Type::I32,
                right: Type::I32,
            },
        });
        self.begin_control(Instruction::If(BlockType::Result(ValType::I32)), None, &[]);
        if cases.len() == 1 {
            self.instructions.push(Instruction::Constant {
                ty: Type::I32,
                bits: u64::from(first_index),
            });
            self.instructions.push(Instruction::Else);
            self.instructions.push(Instruction::Constant {
                ty: Type::I32,
                bits: u64::from(default),
            });
        } else {
            self.sparse_case_index(&cases[..midpoint], first_index, default, selector_slot);
            self.instructions.push(Instruction::Else);
            self.sparse_case_index(
                &cases[midpoint..],
                first_index + midpoint as u32,
                default,
                selector_slot,
            );
        }
        self.end_control();
    }
}

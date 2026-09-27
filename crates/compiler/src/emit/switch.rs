//! Bounded multiway dispatch and case-result emission.
use std::borrow::Cow;

use wasm_encoder::{BlockType, Instruction, ValType};

use super::{Emitter, LocalOp};
use crate::body::{Block, Site, SwitchCase, Target};

impl Emitter<'_> {
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
        let before_arm = self.planner.checkpoint();
        for case in cases {
            self.end_control();
            self.planner.restore(&before_arm);
            self.block(&case.block, None);
            if case.block.terminal.is_none() {
                self.branch_to(Target::exit(site));
            }
        }
        self.end_control();
        self.planner.restore(&before_arm);
        self.block(default, Some(Target::exit(site)));
        self.end_control();
        self.planner.restore(&before_arm);
    }

    fn dispatch_switch(&mut self, cases: &[SwitchCase]) {
        let Some(first) = cases.first() else {
            self.code.instruction(Instruction::Drop);
            return;
        };
        let default = u32::try_from(cases.len()).expect("case count fits the Wasm index space");
        let span = u64::from(cases.last().unwrap().key) - u64::from(first.key) + 1;
        // Permit modest gaps (including opcode sets near six slots per key), but
        // cap tables independently of key magnitude and supplied case count.
        let dense = span <= 4096 && span <= 8 * cases.len() as u64;
        let targets = if dense {
            if first.key != 0 {
                self.code
                    .instruction(Instruction::I32Const(first.key as i32));
                self.code.instruction(Instruction::I32Sub);
            }
            let mut targets = vec![default; span as usize];
            for (index, case) in cases.iter().enumerate() {
                targets[(case.key - first.key) as usize] = index as u32;
            }
            targets
        } else {
            // A sparse selector is still evaluated once. Balanced comparisons
            // map it to a compact case index; no case body or default is copied.
            let selector_slot = self.code.temporary(ValType::I32);
            self.code.local(selector_slot, LocalOp::Set);
            self.sparse_case_index(cases, 0, default, selector_slot);
            (0..default).collect()
        };
        self.code
            .instruction(Instruction::BrTable(Cow::Owned(targets), default));
    }

    fn sparse_case_index(
        &mut self,
        cases: &[SwitchCase],
        first_index: u32,
        default: u32,
        selector_slot: usize,
    ) {
        let midpoint = cases.len() / 2;
        self.code.local(selector_slot, LocalOp::Get);
        self.code
            .instruction(Instruction::I32Const(cases[midpoint].key as i32));
        if cases.len() == 1 {
            self.code.instruction(Instruction::I32Eq);
            self.begin_control(Instruction::If(BlockType::Result(ValType::I32)), None, &[]);
            self.code
                .instruction(Instruction::I32Const(first_index as i32));
            self.code.instruction(Instruction::Else);
            self.code.instruction(Instruction::I32Const(default as i32));
        } else {
            self.code.instruction(Instruction::I32LtU);
            self.begin_control(Instruction::If(BlockType::Result(ValType::I32)), None, &[]);
            self.sparse_case_index(&cases[..midpoint], first_index, default, selector_slot);
            self.code.instruction(Instruction::Else);
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

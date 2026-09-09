//! Bounded multiway dispatch and case-result emission.
use std::borrow::Cow;

use wasm_encoder::{BlockType, Encode, Instruction, ValType};

use super::{LocalOp, Scheduler};
use crate::{
    control::{Region, SwitchCase},
    Terminal,
};

impl Scheduler<'_> {
    pub(super) fn open_switch(&mut self, cases: usize, result: BlockType) {
        // The outer block joins falling-through arms. Each inner block is a case
        // label, followed by the default label. Open them before evaluating the
        // selector so its stack value is inside the innermost label's scope.
        Instruction::Block(result).encode(&mut self.bytes);
        for _ in 0..=cases {
            Instruction::Block(BlockType::Empty).encode(&mut self.bytes);
        }
    }

    pub(super) fn switch(&mut self, cases: &[SwitchCase], default: &Region, yield_result: bool) {
        self.dispatch_switch(cases);
        let before_arm = self.emitted.clone();
        for (index, case) in cases.iter().enumerate() {
            Instruction::End.encode(&mut self.bytes);
            self.emitted.clone_from(&before_arm);
            self.region(&case.region, yield_result);
            if matches!(case.region.terminal, None | Some(Terminal::Yield(_))) {
                let depth = u32::try_from(cases.len() - index)
                    .expect("branch depth fits the Wasm index space");
                Instruction::Br(depth).encode(&mut self.bytes);
            }
        }
        Instruction::End.encode(&mut self.bytes);
        self.emitted.clone_from(&before_arm);
        self.region(default, yield_result);
        Instruction::End.encode(&mut self.bytes);
        self.emitted = before_arm;
    }

    fn dispatch_switch(&mut self, cases: &[SwitchCase]) {
        let Some(first) = cases.first() else {
            Instruction::Drop.encode(&mut self.bytes);
            return;
        };
        let default = u32::try_from(cases.len()).expect("case count fits the Wasm index space");
        let span = u64::from(cases.last().unwrap().key) - u64::from(first.key) + 1;
        // Permit modest gaps (including opcode sets near six slots per key), but
        // cap tables independently of key magnitude and supplied case count.
        let dense = span <= 4096 && span <= 8 * cases.len() as u64;
        let targets = if dense {
            if first.key != 0 {
                Instruction::I32Const(first.key as i32).encode(&mut self.bytes);
                Instruction::I32Sub.encode(&mut self.bytes);
            }
            let mut targets = vec![default; span as usize];
            for (index, case) in cases.iter().enumerate() {
                targets[(case.key - first.key) as usize] = index as u32;
            }
            targets
        } else {
            // A sparse selector is still evaluated once. Balanced comparisons
            // map it to a compact case index; no case body or default is copied.
            let selector_slot = self.placement.slot_types.len();
            self.placement.slot_types.push(ValType::I32);
            self.local(selector_slot, LocalOp::Set);
            self.sparse_case_index(cases, 0, default, selector_slot);
            (0..default).collect()
        };
        Instruction::BrTable(Cow::Owned(targets), default).encode(&mut self.bytes);
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
        Instruction::I32Const(cases[midpoint].key as i32).encode(&mut self.bytes);
        if cases.len() == 1 {
            Instruction::I32Eq.encode(&mut self.bytes);
            Instruction::If(BlockType::Result(ValType::I32)).encode(&mut self.bytes);
            Instruction::I32Const(first_index as i32).encode(&mut self.bytes);
            Instruction::Else.encode(&mut self.bytes);
            Instruction::I32Const(default as i32).encode(&mut self.bytes);
        } else {
            Instruction::I32LtU.encode(&mut self.bytes);
            Instruction::If(BlockType::Result(ValType::I32)).encode(&mut self.bytes);
            self.sparse_case_index(&cases[..midpoint], first_index, default, selector_slot);
            Instruction::Else.encode(&mut self.bytes);
            self.sparse_case_index(
                &cases[midpoint..],
                first_index + midpoint as u32,
                default,
                selector_slot,
            );
        }
        Instruction::End.encode(&mut self.bytes);
    }
}

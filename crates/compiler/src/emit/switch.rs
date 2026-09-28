//! Dense and sparse switch dispatch consume one graph selector.
use super::{Wasm, Writer};
use wasm_encoder::BlockType;
impl Writer<'_> {
    pub(super) fn dispatch(&mut self, keys: &[u32]) {
        let Some(&first) = keys.first() else {
            self.emit(Wasm::Br(0));
            return;
        };
        let default = keys.len() as u32;
        let span = u64::from(*keys.last().unwrap()) - u64::from(first) + 1;
        let targets = if span <= 4096 && span <= 8 * keys.len() as u64 {
            if first != 0 {
                self.emit(Wasm::I32Const(first as i32));
                self.emit(Wasm::I32Sub);
            }
            let mut targets = vec![default; span as usize];
            for (index, &key) in keys.iter().enumerate() {
                targets[(key - first) as usize] = index as u32;
            }
            targets
        } else {
            self.emit(Wasm::LocalSet(self.switch_local));
            self.sparse_index(keys, 0, default);
            (0..default).collect()
        };
        self.emit(Wasm::BrTable(targets.into(), default));
    }
    fn sparse_index(&mut self, keys: &[u32], first: u32, default: u32) {
        let midpoint = keys.len() / 2;
        self.emit(Wasm::LocalGet(self.switch_local));
        self.emit(Wasm::I32Const(keys[midpoint] as i32));
        self.emit(if keys.len() == 1 {
            Wasm::I32Eq
        } else {
            Wasm::I32LtU
        });
        self.emit(Wasm::If(BlockType::Result(wasm_encoder::ValType::I32)));
        if keys.len() == 1 {
            self.emit(Wasm::I32Const(first as i32));
            self.emit(Wasm::Else);
            self.emit(Wasm::I32Const(default as i32));
        } else {
            self.sparse_index(&keys[..midpoint], first, default);
            self.emit(Wasm::Else);
            self.sparse_index(&keys[midpoint..], first + midpoint as u32, default);
        }
        self.emit(Wasm::End);
    }
}

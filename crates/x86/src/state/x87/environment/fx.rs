//! The FXSR header keeps segment:offset pointers and one occupancy bit per register.

use wasm86_compiler::{Val, I32};

use super::Environment;
use crate::SegmentDefaultSize;

fn offset_mask(size: SegmentDefaultSize) -> u32 {
    match size {
        SegmentDefaultSize::Bits16 => 0xffff,
        SegmentDefaultSize::Bits32 => u32::MAX,
    }
}

impl Environment {
    pub(super) fn encode_fx(&self, pointer_size: SegmentDefaultSize) -> [Val<I32>; 6] {
        let mut occupied = Val::<I32>::from(0);
        for index in 0..8 {
            let present = self.tags.unsigned().shr(index * 2).and(3).ne(3);
            occupied = occupied.or(present.unsigned().extend::<I32>().shl(index));
        }
        let opcode = self
            .opcode
            .as_ref()
            .expect("a saved environment has an opcode");
        [
            self.control.unsigned().extend::<I32>().or(self
                .status
                .unsigned()
                .extend::<I32>()
                .shl(16)),
            occupied.or(opcode.and(0x7ff).unsigned().extend::<I32>().shl(16)),
            self.instruction.offset.and(offset_mask(pointer_size)),
            self.instruction.selector.unsigned().extend::<I32>(),
            self.data.offset.and(offset_mask(pointer_size)),
            self.data.selector.unsigned().extend::<I32>(),
        ]
    }
}

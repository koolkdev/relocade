//! The FXSR header keeps segment:offset pointers and one occupancy bit per register.

use wasm86_compiler::{Val, I16, I32};

use super::{Environment, Pointer};
use crate::SegmentDefaultSize;

fn offset_mask(size: SegmentDefaultSize) -> u32 {
    match size {
        SegmentDefaultSize::Bits16 => 0xffff,
        SegmentDefaultSize::Bits32 => u32::MAX,
    }
}

impl Environment {
    pub(super) fn decode_fx(words: &[Val<I32>; 6], pointer_size: SegmentDefaultSize) -> Self {
        let [control_status, tags_opcode, instruction_offset, instruction_selector, data_offset, data_selector] =
            words;
        let mut tags = Val::<I16>::from(0);
        for index in 0..8 {
            let empty = tags_opcode.and(1 << index).eq(0);
            tags = tags.or(empty.select::<I16>(3, 0).shl(index * 2));
        }
        Self {
            control: control_status.truncate::<I16>(),
            status: control_status.unsigned().shr(16).truncate::<I16>(),
            tags,
            opcode: Some(tags_opcode.unsigned().shr(16).and(0x7ff).truncate::<I16>()),
            instruction: Pointer {
                offset: instruction_offset.and(offset_mask(pointer_size)),
                selector: instruction_selector.truncate::<I16>(),
            },
            data: Pointer {
                offset: data_offset.and(offset_mask(pointer_size)),
                selector: data_selector.truncate::<I16>(),
            },
        }
    }

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

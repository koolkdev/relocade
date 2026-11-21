//! Shared fields and pointer encoding for Intel's four legacy environment layouts.

use relocade_compiler::{AtLeast, MemoryInt, Val, I1, I16, I32};

use super::{Environment, Pointer};

const WORD_PADDING: u32 = 0xffff_0000;
// Real-address layouts place address bit 16 at bit 12 of the high pointer word.
// The 14-byte image keeps only the low 16 bits of each encoded word.
const REAL_POINTER_SHIFT: u32 = 16 - 12;
const REAL_POINTER_HIGH: u32 = 0x0fff_f000;
const OPCODE_MASK: u32 = 0x7ff;

impl Pointer {
    fn encode(&self, protected: &Val<I1>) -> (Val<I32>, Val<I32>) {
        let selector = self.selector.unsigned().extend::<I32>();
        let linear = self.offset.add(selector.shl(4));
        (
            protected.select(&self.offset, linear.and(0xffff).or(WORD_PADDING)),
            protected.select(
                selector,
                linear
                    .unsigned()
                    .shr(REAL_POINTER_SHIFT)
                    .and(REAL_POINTER_HIGH),
            ),
        )
    }

    fn decode(low: Val<I32>, high: Val<I32>, protected: &Val<I1>) -> Self {
        let linear = low
            .and(0xffff)
            .or(high.and(REAL_POINTER_HIGH).shl(REAL_POINTER_SHIFT));
        Self {
            offset: protected.select(low, linear),
            // Real-address images retain only the linear address.
            selector: protected.select(high.truncate::<I16>(), 0),
        }
    }
}

impl Environment {
    pub(super) fn encode<T: MemoryInt>(&self, protected: &Val<I1>) -> [Val<T>; 7]
    where
        I32: AtLeast<T>,
    {
        let (instruction_low, instruction_high) = self.instruction.encode(protected);
        let (data_low, data_high) = self.data.encode(protected);
        let opcode = self.opcode.and(OPCODE_MASK).unsigned().extend::<I32>();
        [
            self.control.unsigned().extend::<I32>().or(WORD_PADDING),
            self.status.unsigned().extend::<I32>().or(WORD_PADDING),
            self.tags.unsigned().extend::<I32>().or(WORD_PADDING),
            instruction_low,
            instruction_high.or(protected.select(opcode.shl(16), opcode)),
            data_low,
            data_high.or(protected.select::<I32>(WORD_PADDING, 0)),
        ]
        .map(|word| word.truncate::<T>())
    }

    pub(super) fn decode<T: MemoryInt>(
        words: &[Val<T>; 7],
        protected: &Val<I1>,
        previous_opcode: Val<I16>,
    ) -> Self
    where
        I32: AtLeast<T>,
    {
        let [control, status, tags, instruction_low, instruction_high, data_low, data_high] =
            words.clone().map(|word| word.unsigned().extend::<I32>());
        // The protected 14-byte image has no opcode field.
        let protected_opcode = if T::BYTES == 2 {
            previous_opcode
        } else {
            instruction_high
                .unsigned()
                .shr(16)
                .and(OPCODE_MASK)
                .truncate::<I16>()
        };
        Self {
            control: control.truncate::<I16>(),
            status: status.truncate::<I16>(),
            tags: tags.truncate::<I16>(),
            opcode: protected.select(
                protected_opcode,
                instruction_high.and(OPCODE_MASK).truncate::<I16>(),
            ),
            instruction: Pointer::decode(instruction_low, instruction_high, protected),
            data: Pointer::decode(data_low, data_high, protected),
        }
    }
}

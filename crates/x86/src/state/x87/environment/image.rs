//! Shared fields and pointer encoding for Intel's four legacy environment layouts.

use wasm86_compiler::{AtLeast, MemoryInt, Val, I16, I32};

use super::EnvironmentMode;

pub(super) struct Pointer {
    pub(super) offset: Val<I32>,
    pub(super) selector: Val<I16>,
}

pub(super) struct Environment {
    pub(super) control: Val<I16>,
    pub(super) status: Val<I16>,
    pub(super) tags: Val<I16>,
    /// The protected 14-byte image has no opcode field.
    pub(super) opcode: Option<Val<I16>>,
    pub(super) instruction: Pointer,
    pub(super) data: Pointer,
}

struct PointerWords {
    low: Val<I32>,
    high: Val<I32>,
}

#[derive(Clone, Copy)]
enum PointerEncoding {
    Segmented,
    Linear { high_mask: u32 },
}

struct Layout {
    word_padding: u32,
    pointers: PointerEncoding,
    opcode_shift: Option<u32>,
}

// Real-address layouts place address bit 16 at bit 12 of the high pointer word.
const REAL_POINTER_SHIFT: u32 = 16 - 12;
const OPCODE_MASK: u32 = 0x7ff;

impl Layout {
    fn new(mode: EnvironmentMode, word_bytes: u32) -> Self {
        match (mode, word_bytes) {
            (EnvironmentMode::Protected, 2) => Self {
                word_padding: 0,
                pointers: PointerEncoding::Segmented,
                opcode_shift: None,
            },
            (EnvironmentMode::Protected, 4) => Self {
                word_padding: 0xffff_0000,
                pointers: PointerEncoding::Segmented,
                opcode_shift: Some(16),
            },
            (EnvironmentMode::Real, 2) => Self {
                word_padding: 0,
                pointers: PointerEncoding::Linear { high_mask: 0xf000 },
                opcode_shift: Some(0),
            },
            (EnvironmentMode::Real, 4) => Self {
                word_padding: 0xffff_0000,
                pointers: PointerEncoding::Linear {
                    high_mask: 0x0fff_f000,
                },
                opcode_shift: Some(0),
            },
            _ => unreachable!("environment images use words or dwords"),
        }
    }

    fn encode_word(&self, word: &Val<I16>) -> Val<I32> {
        word.unsigned().extend::<I32>().or(self.word_padding)
    }

    fn encode_pointer(&self, pointer: &Pointer) -> PointerWords {
        match self.pointers {
            PointerEncoding::Segmented => PointerWords {
                low: pointer.offset.clone(),
                high: pointer.selector.unsigned().extend::<I32>(),
            },
            PointerEncoding::Linear { high_mask } => {
                let address = pointer
                    .offset
                    .add(pointer.selector.unsigned().extend::<I32>().shl(4));
                PointerWords {
                    low: address.and(0xffff).or(self.word_padding),
                    high: address.unsigned().shr(REAL_POINTER_SHIFT).and(high_mask),
                }
            }
        }
    }

    fn decode_pointer(&self, words: PointerWords) -> Pointer {
        match self.pointers {
            PointerEncoding::Segmented => Pointer {
                offset: words.low,
                selector: words.high.truncate::<I16>(),
            },
            PointerEncoding::Linear { high_mask } => Pointer {
                offset: words
                    .low
                    .and(0xffff)
                    .or(words.high.and(high_mask).shl(REAL_POINTER_SHIFT)),
                // The image retains only the linear address. A zero selector
                // preserves that address when the environment is saved again.
                selector: 0.into(),
            },
        }
    }

    fn data_pointer_padding(&self) -> u32 {
        match self.pointers {
            PointerEncoding::Segmented => self.word_padding,
            PointerEncoding::Linear { .. } => 0,
        }
    }
}

impl Environment {
    pub(super) fn encode<T: MemoryInt>(&self, mode: EnvironmentMode) -> [Val<T>; 7]
    where
        I32: AtLeast<T>,
    {
        let layout = Layout::new(mode, T::BYTES);
        let instruction = layout.encode_pointer(&self.instruction);
        let data = layout.encode_pointer(&self.data);
        let opcode = self
            .opcode
            .as_ref()
            .zip(layout.opcode_shift)
            .map(|(opcode, shift)| {
                opcode
                    .and(OPCODE_MASK)
                    .unsigned()
                    .extend::<I32>()
                    .shl(shift)
            })
            .unwrap_or_else(|| 0.into());
        [
            layout.encode_word(&self.control),
            layout.encode_word(&self.status),
            layout.encode_word(&self.tags),
            instruction.low,
            instruction.high.or(opcode),
            data.low,
            data.high.or(layout.data_pointer_padding()),
        ]
        .map(|word| word.truncate::<T>())
    }

    pub(super) fn decode<T: MemoryInt>(words: &[Val<T>; 7], mode: EnvironmentMode) -> Self
    where
        I32: AtLeast<T>,
    {
        let layout = Layout::new(mode, T::BYTES);
        let [control, status, tags, instruction_low, instruction_high, data_low, data_high] =
            words.clone().map(|word| word.unsigned().extend::<I32>());
        let opcode = layout.opcode_shift.map(|shift| {
            instruction_high
                .unsigned()
                .shr(shift)
                .and(OPCODE_MASK)
                .truncate::<I16>()
        });
        Self {
            control: control.truncate::<I16>(),
            status: status.truncate::<I16>(),
            tags: tags.truncate::<I16>(),
            opcode,
            instruction: layout.decode_pointer(PointerWords {
                low: instruction_low,
                high: instruction_high,
            }),
            data: layout.decode_pointer(PointerWords {
                low: data_low,
                high: data_high,
            }),
        }
    }
}

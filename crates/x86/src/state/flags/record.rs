//! CPU flag-record layouts and the conversion from symbolic sources at publication.

use wasm86_compiler::{AtLeast, BuildError, FunctionBuilder, Mem, MemoryInt, Val, I1, I32, I8};

use crate::flags::{ArithmeticKind, FlagSource, StatusFlag};

pub(in crate::state) const CONCRETE_KIND: u8 = 0;
pub(in crate::state) const KIND_OFFSET: u32 = 0;
pub(in crate::state) const LEFT_OFFSET: u32 = 4;
pub(in crate::state) const RIGHT_OFFSET: u32 = 8;
pub(in crate::state) const CONCRETE_OFFSET: u32 = 12;

pub(in crate::state) const STATUS_FLAGS: [StatusFlag; 6] = [
    StatusFlag::CF,
    StatusFlag::PF,
    StatusFlag::AF,
    StatusFlag::ZF,
    StatusFlag::SF,
    StatusFlag::OF,
];

pub(in crate::state) fn status_index(flag: StatusFlag) -> usize {
    STATUS_FLAGS
        .iter()
        .position(|candidate| *candidate == flag)
        .expect("every status flag has a CPU byte")
}

pub(in crate::state) fn width_code<T: MemoryInt>() -> u8 {
    match T::BYTES {
        1 => 0,
        2 => 4,
        4 => 8,
        _ => unreachable!("x86 status sources have byte, word or dword operands"),
    }
}

pub(in crate::state) fn encode_kind<T: MemoryInt>(kind: ArithmeticKind) -> u8 {
    width_code::<T>()
        | match kind {
            ArithmeticKind::Sub => 1,
            ArithmeticKind::Add => 2,
        }
}

pub(in crate::state) fn encode_logic<T: MemoryInt>() -> u8 {
    width_code::<T>() | 3
}

/// Each payload determines its valid kind and required CPU fields.
/// A record is constructed only when the current source must be published.
pub(super) enum FlagRecord<T: MemoryInt> {
    Arithmetic {
        operation: ArithmeticKind,
        left: Val<T>,
        right: Val<T>,
    },
    Logic {
        result: Val<T>,
    },
    Concrete {
        status: [Val<I1>; 6],
    },
}

impl<T: MemoryInt> FlagRecord<T> {
    pub(super) fn from_source(source: &FlagSource<T>) -> Self {
        match source {
            FlagSource::Arithmetic {
                kind, left, right, ..
            } => Self::Arithmetic {
                operation: *kind,
                left: left.clone(),
                right: right.clone(),
            },
            FlagSource::Logic { result } => Self::Logic {
                result: result.clone(),
            },
            FlagSource::Explicit { .. } => Self::Concrete {
                status: STATUS_FLAGS.map(|flag| source.flag(flag)),
            },
        }
    }

    fn encoded_kind(&self) -> u8 {
        match self {
            Self::Arithmetic { operation, .. } => encode_kind::<T>(*operation),
            Self::Logic { .. } => encode_logic::<T>(),
            Self::Concrete { .. } => CONCRETE_KIND,
        }
    }

    pub(super) fn write(self, body: &mut FunctionBuilder<'_>, memory: Mem) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
    {
        let kind = self.encoded_kind();
        match self {
            Self::Arithmetic { left, right, .. } => {
                body.store(memory, LEFT_OFFSET, left.unsigned().extend::<I32>())?;
                body.store(memory, RIGHT_OFFSET, right.unsigned().extend::<I32>())?;
            }
            Self::Logic { result } => {
                body.store(memory, LEFT_OFFSET, result.unsigned().extend::<I32>())?;
            }
            Self::Concrete { status } => {
                for (index, value) in status.into_iter().enumerate() {
                    body.store::<I8>(
                        memory,
                        CONCRETE_OFFSET + index as u32,
                        value.unsigned().extend::<I8>(),
                    )?;
                }
            }
        }
        // Publish the tag after every field it describes. Unused fields retain
        // their backing bytes; only the chosen payload is written.
        body.store::<I8>(memory, KIND_OFFSET, u32::from(kind))
    }
}

//! Conversion from symbolic flags to stored records at publication.

use wasm86_compiler::{AtLeast, BuildError, FunctionBuilder, Mem, MemoryInt, Val, I1, I32, I8};

use crate::flags::{ArithmeticKind, FlagSource};
use crate::state::access::cpu_store;

pub(in crate::state) const CONCRETE_KIND: u8 = 0;

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
            FlagSource::Explicit { flags, .. } => Self::Concrete {
                status: flags.clone(),
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
                cpu_store!(body, memory, flags.left, left.unsigned().extend::<I32>())?;
                cpu_store!(body, memory, flags.right, right.unsigned().extend::<I32>())?;
            }
            Self::Logic { result } => {
                cpu_store!(body, memory, flags.left, result.unsigned().extend::<I32>())?;
            }
            Self::Concrete { status } => {
                let [cf, pf, af, zf, sf, of] = status;
                cpu_store!(body, memory, flags.status.cf, cf.unsigned().extend::<I8>())?;
                cpu_store!(body, memory, flags.status.pf, pf.unsigned().extend::<I8>())?;
                cpu_store!(body, memory, flags.status.af, af.unsigned().extend::<I8>())?;
                cpu_store!(body, memory, flags.status.zf, zf.unsigned().extend::<I8>())?;
                cpu_store!(body, memory, flags.status.sf, sf.unsigned().extend::<I8>())?;
                cpu_store!(body, memory, flags.status.of, of.unsigned().extend::<I8>())?;
            }
        }
        // Publish the tag after every field it describes. Unused fields retain
        // their backing bytes; only the chosen payload is written.
        cpu_store!(body, memory, flags.kind, u32::from(kind))
    }
}

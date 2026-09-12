//! Conversion from symbolic flags to stored records at publication.

use wasm86_compiler::{AtLeast, BuildError, FunctionBuilder, Mem, MemoryInt, Val, I1, I32, I8};

use crate::alu::{ArithmeticOp, StatusSource};
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

pub(in crate::state) fn encode_kind<T: MemoryInt>(operation: ArithmeticOp) -> u8 {
    width_code::<T>()
        | match operation {
            ArithmeticOp::Subtract => 1,
            ArithmeticOp::Add => 2,
        }
}

pub(in crate::state) fn encode_logic<T: MemoryInt>() -> u8 {
    width_code::<T>() | 3
}

/// Each payload determines its valid kind and required CPU fields.
/// A record is constructed only when the current source must be published.
pub(super) enum FlagRecord<T: MemoryInt> {
    Arithmetic {
        operation: ArithmeticOp,
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
    pub(super) fn from_source(source: &StatusSource<T>) -> Self {
        match source {
            StatusSource::Arithmetic {
                operation,
                left,
                right,
                ..
            } => Self::Arithmetic {
                operation: *operation,
                left: left.clone(),
                right: right.clone(),
            },
            StatusSource::Logic { result } => Self::Logic {
                result: result.clone(),
            },
            StatusSource::Explicit { flags } => Self::Concrete {
                status: flags.clone(),
            },
        }
    }

    pub(super) fn write(self, body: &mut FunctionBuilder<'_>, memory: Mem) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
    {
        let kind = match self {
            Self::Arithmetic {
                operation,
                left,
                right,
            } => {
                cpu_store!(
                    body,
                    memory,
                    flags.status_source.left,
                    left.unsigned().extend::<I32>()
                )?;
                cpu_store!(
                    body,
                    memory,
                    flags.status_source.right,
                    right.unsigned().extend::<I32>()
                )?;
                encode_kind::<T>(operation)
            }
            Self::Logic { result } => {
                cpu_store!(
                    body,
                    memory,
                    flags.status_source.left,
                    result.unsigned().extend::<I32>()
                )?;
                encode_logic::<T>()
            }
            Self::Concrete { status } => return write_concrete(body, memory, status),
        };
        // Publish the tag after every field it describes. Unused fields retain
        // their backing bytes; only the chosen payload is written.
        cpu_store!(body, memory, flags.status_source.kind, u32::from(kind))
    }
}

pub(super) fn write_concrete(
    body: &mut FunctionBuilder<'_>,
    memory: Mem,
    status: [Val<I1>; 6],
) -> Result<(), BuildError> {
    let [cf, pf, af, zf, sf, of] = status;
    cpu_store!(body, memory, flags.bytes.cf, cf.unsigned().extend::<I8>())?;
    cpu_store!(body, memory, flags.bytes.pf, pf.unsigned().extend::<I8>())?;
    cpu_store!(body, memory, flags.bytes.af, af.unsigned().extend::<I8>())?;
    cpu_store!(body, memory, flags.bytes.zf, zf.unsigned().extend::<I8>())?;
    cpu_store!(body, memory, flags.bytes.sf, sf.unsigned().extend::<I8>())?;
    cpu_store!(body, memory, flags.bytes.of, of.unsigned().extend::<I8>())?;
    cpu_store!(
        body,
        memory,
        flags.status_source.kind,
        u32::from(CONCRETE_KIND)
    )
}

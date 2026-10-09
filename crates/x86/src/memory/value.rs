//! Transfer decomposition keeps callback carriers separate from guest value width.

use wasm86_compiler::{BuildError, MemoryType, Val, I16, I32, I64, I8, V128};

/// Guest values that can be transferred through integer parts of at most eight bytes.
/// Callbacks receive the relative byte offset and part width, plus the value for
/// writes. Parts run in increasing byte order and retain live physical routing.
pub(crate) trait TransferType: MemoryType {
    fn read_parts(
        read: impl FnMut(u32, u32) -> Result<Val<I64>, BuildError>,
    ) -> Result<Val<Self>, BuildError>;
    fn write_parts(
        value: &Val<Self>,
        write: impl FnMut(u32, u32, Val<I64>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError>;
}

macro_rules! scalar {
    ($($ty:ty),+) => { $(
        impl TransferType for $ty {
            fn read_parts(mut read: impl FnMut(u32, u32) -> Result<Val<I64>, BuildError>) -> Result<Val<Self>, BuildError> {
                Ok(read(0, Self::BYTES)?.truncate())
            }
            fn write_parts(value: &Val<Self>, mut write: impl FnMut(u32, u32, Val<I64>) -> Result<(), BuildError>) -> Result<(), BuildError> {
                write(0, Self::BYTES, value.unsigned().extend())
            }
        }
    )+};
}
scalar!(I8, I16, I32, I64);

impl TransferType for V128 {
    fn read_parts(
        mut read: impl FnMut(u32, u32) -> Result<Val<I64>, BuildError>,
    ) -> Result<Val<Self>, BuildError> {
        let low = read(0, 8)?;
        let high = read(8, 8)?;
        Ok(Val::<V128>::from(0_u128)
            .replace_lane(0, low)
            .replace_lane(1, high))
    }
    fn write_parts(
        value: &Val<Self>,
        mut write: impl FnMut(u32, u32, Val<I64>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        write(0, 8, value.extract_lane(0))?;
        write(8, 8, value.extract_lane(1))
    }
}

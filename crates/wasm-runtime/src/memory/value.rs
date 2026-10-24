//! Portable little-endian scalar representations, independent of any engine.

/// The unsigned scalar widths supported by linear-memory access.
/// This trait is sealed; it describes bytes, not the engine that transfers them.
pub trait MemoryValue: sealed::Sealed + Copy {
    type Bytes: AsRef<[u8]> + AsMut<[u8]> + Default;

    fn from_le_bytes(bytes: Self::Bytes) -> Self;
    fn to_le_bytes(self) -> Self::Bytes;
}

mod sealed {
    pub trait Sealed {}
}

macro_rules! value {
    ($type:ty, $len:literal) => {
        impl sealed::Sealed for $type {}

        impl MemoryValue for $type {
            type Bytes = [u8; $len];

            #[inline]
            fn from_le_bytes(bytes: Self::Bytes) -> Self {
                Self::from_le_bytes(bytes)
            }

            #[inline]
            fn to_le_bytes(self) -> Self::Bytes {
                self.to_le_bytes()
            }
        }
    };
}

value!(u8, 1);
value!(u16, 2);
value!(u32, 4);
value!(u64, 8);

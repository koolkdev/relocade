use std::marker::PhantomData;

/// An address in the guest process. The pointee's layout and access permissions
/// matter only when the service actually accesses guest memory.
#[repr(transparent)]
pub struct GuestPtr<T> {
    address: u32,
    pointee: PhantomData<fn() -> T>,
}

impl<T> GuestPtr<T> {
    pub const fn new(address: u32) -> Self {
        Self {
            address,
            pointee: PhantomData,
        }
    }

    pub const fn address(self) -> u32 {
        self.address
    }
}

impl<T> Copy for GuestPtr<T> {}
impl<T> Clone for GuestPtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

/// Little-endian stack representation, including padding to a DWORD slot.
/// Slot must be a fixed-size byte array covering the complete parameter.
pub trait Argument: Sized {
    type Slot: AsRef<[u8]> + AsMut<[u8]> + Default;
    const STACK_BYTES: u32 = size_of::<Self::Slot>() as u32;
    fn encode(&self) -> Self::Slot;
    fn decode(slot: Self::Slot) -> Self;
}

/// A type as passed after the C default argument promotions.
pub trait Promoted: Argument {}

/// General-purpose register writes made by a scalar or void return.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReturnRegisters {
    Void,
    Eax(u32),
    EdxEax { edx: u32, eax: u32 },
}

impl ReturnRegisters {
    /// Publishes only the registers used by this result to the live CPU.
    pub fn publish(self, set_eax: impl FnOnce(u32), set_edx: impl FnOnce(u32)) {
        match self {
            Self::Void => {}
            Self::Eax(eax) => set_eax(eax),
            Self::EdxEax { edx, eax } => {
                set_eax(eax);
                set_edx(edx);
            }
        }
    }
}

/// Integer, pointer and void function results in the Microsoft x86 ABI.
pub trait ReturnValue: Sized {
    fn into_registers(self) -> ReturnRegisters;
    fn from_registers(eax: u32, edx: u32) -> Self;
}

macro_rules! word {
    ($($ty:ty),+) => {$(
        impl Argument for $ty {
            type Slot = [u8; 4];
            fn encode(&self) -> Self::Slot {
                (*self as u32).to_le_bytes()
            }
            fn decode(slot: Self::Slot) -> Self {
                u32::from_le_bytes(slot) as Self
            }
        }

        impl ReturnValue for $ty {
            fn into_registers(self) -> ReturnRegisters {
                ReturnRegisters::Eax(self as u32)
            }
            fn from_registers(eax: u32, _: u32) -> Self {
                eax as Self
            }
        }
    )+};
}

word!(i8, u8, i16, u16, i32, u32);

macro_rules! full_slot {
    ($($ty:ty),+) => {$(
        impl Argument for $ty {
            type Slot = [u8; size_of::<$ty>()];
            fn encode(&self) -> Self::Slot {
                self.to_le_bytes()
            }
            fn decode(slot: Self::Slot) -> Self {
                Self::from_le_bytes(slot)
            }
        }
    )+};
}

full_slot!(u64, i64, f32, f64);

impl ReturnValue for u64 {
    fn into_registers(self) -> ReturnRegisters {
        ReturnRegisters::EdxEax {
            edx: (self >> 32) as u32,
            eax: self as u32,
        }
    }
    fn from_registers(eax: u32, edx: u32) -> Self {
        u64::from(edx) << 32 | u64::from(eax)
    }
}

impl ReturnValue for i64 {
    fn into_registers(self) -> ReturnRegisters {
        (self as u64).into_registers()
    }
    fn from_registers(eax: u32, edx: u32) -> Self {
        u64::from_registers(eax, edx) as i64
    }
}

impl ReturnValue for () {
    fn into_registers(self) -> ReturnRegisters {
        ReturnRegisters::Void
    }
    fn from_registers(_: u32, _: u32) -> Self {}
}

impl<T> Argument for GuestPtr<T> {
    type Slot = [u8; 4];
    fn encode(&self) -> Self::Slot {
        self.address.to_le_bytes()
    }
    fn decode(slot: Self::Slot) -> Self {
        Self::new(u32::from_le_bytes(slot))
    }
}

impl<T> ReturnValue for GuestPtr<T> {
    fn into_registers(self) -> ReturnRegisters {
        self.address.into_registers()
    }
    fn from_registers(eax: u32, _: u32) -> Self {
        Self::new(eax)
    }
}

impl Promoted for i32 {}
impl Promoted for u32 {}
impl Promoted for i64 {}
impl Promoted for u64 {}
impl Promoted for f64 {}
impl<T> Promoted for GuestPtr<T> {}

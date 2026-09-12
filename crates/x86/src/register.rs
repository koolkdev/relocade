use std::marker::PhantomData;

use wasm86_compiler::{Val, I1, I16, I32, I8};

use crate::ssa::SsaType;

/// A general-purpose register, independent of its position in CPU backing memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Gpr32 {
    Eax,
    Ecx,
    Edx,
    Ebx,
    Esp,
    Ebp,
    Esi,
    Edi,
}

impl Gpr32 {
    /// General-purpose registers in x86 encoding order.
    pub const ALL: [Self; 8] = [
        Self::Eax,
        Self::Ecx,
        Self::Edx,
        Self::Ebx,
        Self::Esp,
        Self::Ebp,
        Self::Esi,
        Self::Edi,
    ];

    /// Decode the low three bits of an x86 register code.
    pub fn from_code(code: u8) -> Self {
        match code & 7 {
            0 => Self::Eax,
            1 => Self::Ecx,
            2 => Self::Edx,
            3 => Self::Ebx,
            4 => Self::Esp,
            5 => Self::Ebp,
            6 => Self::Esi,
            7 => Self::Edi,
            _ => unreachable!(),
        }
    }
}

/// A named parent view or legacy high byte, independent of register encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct NamedRegister {
    parent: Gpr32,
    byte: u32,
}

impl NamedRegister {
    pub(super) const AH: Self = Self {
        parent: Gpr32::Eax,
        byte: 1,
    };

    /// The handler's logical width selects the low part of this parent.
    pub(super) const fn low(parent: Gpr32) -> Self {
        Self { parent, byte: 0 }
    }

    pub(super) const fn same_location(self, other: Self) -> bool {
        self.parent as u8 == other.parent as u8 && self.byte == other.byte
    }
}

impl From<Gpr32> for NamedRegister {
    fn from(parent: Gpr32) -> Self {
        Self::low(parent)
    }
}

/// An x86 register code whose low three bits are interpreted at the operand width.
#[derive(Clone)]
pub(super) enum RegisterCode {
    Known(u8),
    Indexed(Val<I32>),
}

impl RegisterCode {
    pub(super) fn from_code(code: u8) -> Self {
        Self::Known(code)
    }

    pub(super) fn indexed(code: Val<I32>) -> Self {
        Self::Indexed(code)
    }

    pub(super) fn view<T: RegisterType>(self) -> Register<T> {
        Register {
            selection: T::select(self),
            marker: PhantomData,
        }
    }
}

/// A register operand has a named view or an encoded register field.
#[derive(Clone)]
pub(super) enum RegisterOperand {
    Named(NamedRegister),
    Encoded(RegisterCode),
}

impl RegisterOperand {
    pub(super) fn view<T: RegisterType>(self) -> Register<T> {
        match self {
            Self::Named(register) => Register::named(register),
            Self::Encoded(code) => code.view(),
        }
    }
}

impl From<Gpr32> for RegisterOperand {
    fn from(parent: Gpr32) -> Self {
        Self::Named(parent.into())
    }
}

impl From<NamedRegister> for RegisterOperand {
    fn from(register: NamedRegister) -> Self {
        Self::Named(register)
    }
}

impl From<RegisterCode> for RegisterOperand {
    fn from(code: RegisterCode) -> Self {
        Self::Encoded(code)
    }
}

#[derive(Clone)]
pub(super) enum RegisterSelection {
    Named {
        parent: Gpr32,
        byte: u32,
    },
    Indexed {
        slot: Val<I32>,
        byte: Option<Val<I32>>,
    },
}

pub(super) trait RegisterType: SsaType {
    /// Number of parent slots reachable by an indexed encoded register.
    const BACKING_SLOT_COUNT: u32;

    fn select(code: RegisterCode) -> RegisterSelection;
}

impl RegisterType for I8 {
    const BACKING_SLOT_COUNT: u32 = 4;

    fn select(code: RegisterCode) -> RegisterSelection {
        match code {
            RegisterCode::Known(code) => RegisterSelection::Named {
                parent: Gpr32::from_code(code & 3),
                byte: u32::from((code >> 2) & 1),
            },
            RegisterCode::Indexed(code) => RegisterSelection::Indexed {
                slot: code.and(3),
                byte: Some(code.unsigned().shr(2).and(1)),
            },
        }
    }
}

impl RegisterSelection {
    fn at_slot_start(code: RegisterCode) -> Self {
        match code {
            RegisterCode::Known(code) => Self::Named {
                parent: Gpr32::from_code(code),
                byte: 0,
            },
            RegisterCode::Indexed(code) => Self::Indexed {
                slot: code.and(7),
                byte: None,
            },
        }
    }
}

impl RegisterType for I16 {
    const BACKING_SLOT_COUNT: u32 = 8;

    fn select(code: RegisterCode) -> RegisterSelection {
        RegisterSelection::at_slot_start(code)
    }
}

impl RegisterType for I32 {
    const BACKING_SLOT_COUNT: u32 = 8;

    fn select(code: RegisterCode) -> RegisterSelection {
        RegisterSelection::at_slot_start(code)
    }
}

#[derive(Clone)]
pub(super) struct Register<T: RegisterType> {
    pub(super) selection: RegisterSelection,
    marker: PhantomData<T>,
}

impl<T: RegisterType> Register<T> {
    /// Selects a named view without decoding register bits. High bytes require I8.
    pub(super) fn named(register: impl Into<NamedRegister>) -> Self {
        let NamedRegister { parent, byte } = register.into();
        assert!(
            byte == 0 || T::BYTES == 1,
            "a high-byte register has byte width"
        );
        Self {
            selection: RegisterSelection::Named { parent, byte },
            marker: PhantomData,
        }
    }

    pub(super) fn indexed(code: Val<I32>) -> Self {
        RegisterCode::indexed(code).view()
    }
}

impl Register<I32> {
    pub(super) fn known(&self) -> Option<Gpr32> {
        match self.selection {
            RegisterSelection::Named { parent, .. } => Some(parent),
            RegisterSelection::Indexed { .. } => None,
        }
    }

    pub(super) fn is(&self, register: Gpr32) -> Val<I1> {
        match &self.selection {
            RegisterSelection::Named { parent, .. } => (*parent == register).into(),
            RegisterSelection::Indexed { slot, .. } => slot.eq(register as u32),
        }
    }
}

impl From<Gpr32> for Register<I32> {
    fn from(parent: Gpr32) -> Self {
        Self::named(parent)
    }
}

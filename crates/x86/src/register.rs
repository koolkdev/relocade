use std::marker::PhantomData;

use wasm86_compiler::{Val, I16, I32, I8};

use crate::ssa::SsaType;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Gpr32 {
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
    pub(super) fn from_code(code: u8) -> Self {
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

/// A register view interprets the low three encoding bits for its operand width.
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
    pub(super) fn indexed(code: Val<I32>) -> Self {
        RegisterCode::indexed(code).view()
    }
}

impl From<Gpr32> for Register<I32> {
    fn from(parent: Gpr32) -> Self {
        Self {
            selection: RegisterSelection::Named { parent, byte: 0 },
            marker: PhantomData,
        }
    }
}

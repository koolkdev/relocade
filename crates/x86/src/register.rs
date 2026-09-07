use wasm86_compiler::{Val, I32};

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

pub(super) enum Register32 {
    Named(Gpr32),
    Indexed(Val<I32>),
}

impl Register32 {
    pub(super) fn indexed(index: Val<I32>) -> Self {
        // Only the low three bits select a 32-bit general-purpose register.
        Self::Indexed(index.and(7))
    }
}

impl From<Gpr32> for Register32 {
    fn from(register: Gpr32) -> Self {
        Self::Named(register)
    }
}

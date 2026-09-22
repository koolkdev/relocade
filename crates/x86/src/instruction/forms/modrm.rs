//! Form selection separates fixed ModRM bits from permitted addressing modes.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModRmMode {
    Any,
    Memory,
    Register,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ModRmSelector {
    pub(crate) mask: u8,
    value: u8,
    mode: ModRmMode,
}

impl ModRmSelector {
    pub(in crate::instruction) const fn any() -> Self {
        Self {
            mask: 0,
            value: 0,
            mode: ModRmMode::Any,
        }
    }

    pub(in crate::instruction) const fn extension(extension: u8) -> Self {
        assert!(extension < 8, "a ModRM extension has three bits");
        Self {
            mask: 0x38,
            value: extension << 3,
            mode: ModRmMode::Any,
        }
    }

    pub(in crate::instruction) const fn byte(byte: u8) -> Self {
        Self {
            mask: 0xff,
            value: byte,
            mode: if byte >> 6 == 3 {
                ModRmMode::Register
            } else {
                ModRmMode::Memory
            },
        }
    }

    pub(in crate::instruction) const fn memory(self) -> Self {
        assert!(
            !matches!(self.mode, ModRmMode::Register),
            "a register encoding cannot bind a memory operand"
        );
        Self {
            mode: ModRmMode::Memory,
            ..self
        }
    }

    pub(crate) fn accepts_memory(self) -> bool {
        !matches!(self.mode, ModRmMode::Register)
    }

    pub(crate) fn accepts_register(self) -> bool {
        !matches!(self.mode, ModRmMode::Memory)
    }

    pub(crate) fn matches(self, byte: u8) -> bool {
        byte & self.mask == self.value
            && if byte >> 6 == 3 {
                self.accepts_register()
            } else {
                self.accepts_memory()
            }
    }
}

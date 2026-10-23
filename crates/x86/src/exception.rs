use std::fmt;

/// Architectural faults reported by execution, checked fetches and host resolution.
/// Payloads use architectural values. The caller supplies the restart and commit
/// boundary independently of the exception's contents.
#[derive(
    relocade_wasm_bindings::WasmAbi,
    relocade_compiler::abi::CompilerValue,
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
)]
pub enum Exception {
    DivideError,
    BoundRangeExceeded,
    InvalidOpcode,
    SegmentNotPresent {
        error_code: u32,
    },
    StackFault {
        error_code: u32,
    },
    GeneralProtection {
        error_code: u32,
    },
    PageFault {
        linear_address: u32,
        error_code: u32,
    },
    /// A pending x87 floating-point exception reported by a waiting instruction.
    FloatingPoint,
    /// An unmasked SIMD floating-point exception at the current instruction.
    SimdFloatingPoint,
}

/// Architectural vector numbers, independent of the host exit encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ExceptionVector {
    DivideError = 0,
    BoundRangeExceeded = 5,
    InvalidOpcode = 6,
    SegmentNotPresent = 11,
    StackFault = 12,
    GeneralProtection = 13,
    PageFault = 14,
    FloatingPoint = 16,
    SimdFloatingPoint = 19,
}

impl Exception {
    pub const fn vector(&self) -> ExceptionVector {
        match self {
            Self::DivideError => ExceptionVector::DivideError,
            Self::BoundRangeExceeded => ExceptionVector::BoundRangeExceeded,
            Self::InvalidOpcode => ExceptionVector::InvalidOpcode,
            Self::SegmentNotPresent { .. } => ExceptionVector::SegmentNotPresent,
            Self::StackFault { .. } => ExceptionVector::StackFault,
            Self::GeneralProtection { .. } => ExceptionVector::GeneralProtection,
            Self::PageFault { .. } => ExceptionVector::PageFault,
            Self::FloatingPoint => ExceptionVector::FloatingPoint,
            Self::SimdFloatingPoint => ExceptionVector::SimdFloatingPoint,
        }
    }
}

impl fmt::Display for Exception {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (name, error_code) = match self {
            Self::DivideError => return f.write_str("#DE"),
            Self::BoundRangeExceeded => return f.write_str("#BR"),
            Self::InvalidOpcode => return f.write_str("#UD"),
            Self::FloatingPoint => return f.write_str("#MF"),
            Self::SimdFloatingPoint => return f.write_str("#XM"),
            Self::SegmentNotPresent { error_code } => ("#NP", error_code),
            Self::StackFault { error_code } => ("#SS", error_code),
            Self::GeneralProtection { error_code } => ("#GP", error_code),
            Self::PageFault {
                linear_address,
                error_code,
            } => return write!(f, "#PF({error_code:#x}) at {linear_address:#x}"),
        };
        write!(f, "{name}({error_code:#x})")
    }
}

impl std::error::Error for Exception {}

#[cfg(test)]
mod tests;

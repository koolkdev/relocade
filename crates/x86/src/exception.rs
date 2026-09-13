use wasm86_compiler::{Val, I32};

/// Architectural faults currently reported by execution and checked instruction fetch.
/// Payloads describe the exception; the caller supplies its restart boundary.
pub(crate) enum Exception {
    DivideError,
    StackFault {
        error_code: Val<I32>,
    },
    GeneralProtection {
        error_code: Val<I32>,
    },
    PageFault {
        linear_address: Val<I32>,
        error_code: Val<I32>,
    },
}

/// Architectural vector numbers, independent of the host exit encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum ExceptionVector {
    DivideError = 0,
    StackFault = 12,
    GeneralProtection = 13,
    PageFault = 14,
}

impl Exception {
    pub(crate) fn vector(&self) -> ExceptionVector {
        match self {
            Self::DivideError => ExceptionVector::DivideError,
            Self::StackFault { .. } => ExceptionVector::StackFault,
            Self::GeneralProtection { .. } => ExceptionVector::GeneralProtection,
            Self::PageFault { .. } => ExceptionVector::PageFault,
        }
    }
}

#[cfg(test)]
mod tests;

use std::fmt;

/// Architectural faults reported by execution, checked fetches and host resolution.
/// Payloads use concrete `u32` values by default; generated execution uses compiler
/// values. The caller supplies the restart and commit boundary, independently of
/// the exception's contents and the host exit encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Exception<V = u32> {
    DivideError,
    BoundRangeExceeded,
    InvalidOpcode,
    SegmentNotPresent {
        error_code: V,
    },
    StackFault {
        error_code: V,
    },
    GeneralProtection {
        error_code: V,
    },
    PageFault {
        linear_address: V,
        error_code: V,
    },
    /// A pending x87 floating-point exception reported by a waiting instruction.
    FloatingPoint,
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
}

impl<V> Exception<V> {
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
        }
    }
}

impl fmt::Display for Exception<u32> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (name, error_code) = match self {
            Self::DivideError => return f.write_str("#DE"),
            Self::BoundRangeExceeded => return f.write_str("#BR"),
            Self::InvalidOpcode => return f.write_str("#UD"),
            Self::FloatingPoint => return f.write_str("#MF"),
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

impl std::error::Error for Exception<u32> {}

#[cfg(test)]
mod tests;

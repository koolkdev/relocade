use std::fmt;

/// Architectural faults reported by execution, checked fetches and host resolution.
/// Payloads use concrete `u32` values by default; generated execution uses compiler
/// values. The caller supplies the restart and commit boundary, independently of
/// the exception's contents and the host exit encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Exception<V = u32> {
    DivideError,
    SegmentNotPresent { error_code: V },
    StackFault { error_code: V },
    GeneralProtection { error_code: V },
    PageFault { linear_address: V, error_code: V },
}

/// Architectural vector numbers, independent of the host exit encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ExceptionVector {
    DivideError = 0,
    SegmentNotPresent = 11,
    StackFault = 12,
    GeneralProtection = 13,
    PageFault = 14,
}

impl<V> Exception<V> {
    pub const fn vector(&self) -> ExceptionVector {
        match self {
            Self::DivideError => ExceptionVector::DivideError,
            Self::SegmentNotPresent { .. } => ExceptionVector::SegmentNotPresent,
            Self::StackFault { .. } => ExceptionVector::StackFault,
            Self::GeneralProtection { .. } => ExceptionVector::GeneralProtection,
            Self::PageFault { .. } => ExceptionVector::PageFault,
        }
    }
}

impl fmt::Display for Exception<u32> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (name, error_code) = match self {
            Self::DivideError => return f.write_str("#DE"),
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
